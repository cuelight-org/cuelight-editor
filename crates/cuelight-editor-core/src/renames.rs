//! Renames: a trigger, a variable or a value given a new name wherever
//! the show and its driver use it, as one undo step.
//!
//! The show is walked by what its keys mean, not searched as text: a
//! timeline named like its trigger keeps its name, and a `map` that
//! happens to list the name is left alone. A trigger is used by a
//! scene, a timeline on a layer or a value (`trigger`, `on_end`), a
//! sound or video (`trigger`, `stop`, `on_end`), a reel's `spin`, a
//! press, a key and the driver's steps; a field that takes a list is
//! renamed in the list. A variable or a value is used by its
//! declaration, every reading of it (a binding, a `when`, a `while`),
//! the pointer and the driver's `set` steps. Variables and values share
//! their names, since a variable takes over the value it is named
//! like, so a rename of either renames both.
//!
//! The driver is a document of its own, parsed the same way so it keeps
//! its layout, and its new text goes into the show's document as an
//! edit of another file: the one undo step takes back both.

use serde_json::Value;

use crate::document::{Document, EditError, Part, Pointer};

/// The show's driver, by its path in the show.
pub const DRIVER: &str = "test-driver.json";

/// What is renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Trigger,
    Variable,
    Value,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Trigger => "trigger",
            Kind::Variable => "variable",
            Kind::Value => "value",
        }
    }
}

/// A place a name is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    /// The file: `show.json`, or the driver.
    pub file: &'static str,
    pub at: Pointer,
    /// The name is the key at `at` (a declaration, a driver's `set`),
    /// not the text there.
    pub key: bool,
    /// What it is, for a person: `timeline enter on layer gauge`.
    pub what: String,
}

/// A rename worked out, to be looked at before it is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub kind: Kind,
    pub from: String,
    pub to: String,
    pub uses: Vec<Use>,
}

/// Every place the show and its driver use `name` as a `kind`.
pub fn uses(kind: Kind, name: &str, show: &Value, driver: Option<&Value>) -> Vec<Use> {
    let mut walk = Walk {
        trigger: kind == Kind::Trigger,
        name,
        file: "show.json",
        out: Vec::new(),
    };
    walk.show(show);
    if let Some(driver) = driver {
        walk.file = DRIVER;
        walk.driver(driver);
    }
    walk.out
}

/// Work out renaming `from` to `to`: what it changes, or why it is
/// refused. A name already in use is refused, as is one nothing uses.
pub fn plan(
    kind: Kind,
    from: &str,
    to: &str,
    show: &Value,
    driver: Option<&Value>,
) -> Result<Plan, String> {
    if to.trim().is_empty() || to.trim() != to {
        return Err("a name is needed, without spaces round it".to_owned());
    }
    if to == from {
        return Err(format!("{from} has that name already"));
    }
    let taken = match kind {
        Kind::Trigger => !uses(kind, to, show, driver).is_empty(),
        Kind::Variable | Kind::Value => {
            declared(show, "variables", to)
                || declared(show, "values", to)
                || !uses(kind, to, show, driver).is_empty()
        }
    };
    if taken {
        return Err(format!("the show uses {to} already"));
    }
    let uses = uses(kind, from, show, driver);
    if uses.is_empty() {
        return Err(format!("nothing uses {from}"));
    }
    Ok(Plan {
        kind,
        from: from.to_owned(),
        to: to.to_owned(),
        uses,
    })
}

/// Make the rename, as one undo step. `driver` is the driver's text as
/// it stands, when the show has one; its new text goes into the
/// document as an edit of [`DRIVER`]. Nothing is changed on an error.
pub fn apply(plan: &Plan, document: &mut Document, driver: Option<&str>) -> Result<(), String> {
    let steps = document.steps().len();
    document.begin_step();
    let done = (|| -> Result<(), String> {
        let mut renamed = None;
        for found in &plan.uses {
            let target = if found.file == DRIVER {
                let Some(text) = driver else {
                    return Err("the driver is not there".to_owned());
                };
                if renamed.is_none() {
                    renamed = Some(Document::parse(text).map_err(|e| format!("{DRIVER}: {e}"))?);
                }
                renamed.as_mut().ok_or("the driver is not there")?
            } else {
                &mut *document
            };
            rename_at(target, found, &plan.to).map_err(|e| e.to_string())?;
        }
        if let (Some(renamed), Some(text)) = (renamed, driver) {
            document.set_file(DRIVER, text, renamed.text());
        }
        Ok(())
    })();
    document.end_step();
    if done.is_err() && document.steps().len() > steps {
        // Take back what was made before the error, and keep no redo.
        document.undo();
        document.begin_step();
        document.end_step();
    }
    done
}

/// The driver's text as the document has it now: as a rename left it,
/// or as the show shipped it.
pub fn driver_text(
    document: &Document,
    files: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Option<String> {
    match document.file(DRIVER) {
        Some(text) => Some(text.to_owned()),
        None => files
            .get(DRIVER)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
    }
}

/// The driver as the edits left it, to play by; `None` when no edit
/// touched it, or what they left does not read as one.
pub fn edited_driver(document: &Document) -> Option<cuelight_loader::Driver> {
    cuelight_loader::Driver::from_json(document.file(DRIVER)?).ok()
}

fn rename_at(document: &mut Document, found: &Use, to: &str) -> Result<(), EditError> {
    if found.key {
        document.rename(&found.at, to)
    } else {
        document.set(&found.at, Value::from(to))
    }
}

fn declared(show: &Value, list: &str, name: &str) -> bool {
    show.get(list).and_then(|l| l.get(name)).is_some()
}

/// The walk: whether it looks for a trigger or a variable's name, and
/// what it found.
struct Walk<'a> {
    trigger: bool,
    name: &'a str,
    file: &'static str,
    out: Vec<Use>,
}

impl Walk<'_> {
    fn found(&mut self, at: Pointer, key: bool, what: String) {
        self.out.push(Use {
            file: self.file,
            at,
            key,
            what,
        });
    }

    /// A field that takes a trigger, or a list of them.
    fn triggers(&mut self, value: Option<&Value>, at: Pointer, what: &str) {
        match value {
            Some(Value::String(name)) if name == self.name => {
                self.found(at, false, what.to_owned())
            }
            Some(Value::Array(names)) => {
                for (i, name) in names.iter().enumerate() {
                    if name.as_str() == Some(self.name) {
                        self.found(at.then(Part::Index(i)), false, what.to_owned());
                    }
                }
            }
            _ => {}
        }
    }

    /// A reading: a binding, a `when` or a `while`.
    fn reading(&mut self, value: Option<&Value>, at: Pointer, what: String) {
        if let Some(reading) = value
            && reading.get("variable").and_then(Value::as_str) == Some(self.name)
        {
            self.found(at.then(key("variable")), false, what);
        }
    }

    fn show(&mut self, show: &Value) {
        let root = Pointer::default();
        let input = root.then(key("input"));
        if self.trigger {
            if let Some(Value::Object(keys)) = show.pointer("/input/keys") {
                for (name, trigger) in keys {
                    let at = input.then(key("keys")).then(key(name));
                    let shown = if name == " " { "Space" } else { name };
                    self.triggers(Some(trigger), at, &format!("key {shown}"));
                }
            }
            self.triggers(
                show.pointer("/input/press"),
                input.then(key("press")),
                "a press on nothing pressable",
            );
        } else {
            for list in ["variables", "values"] {
                if declared(show, list, self.name) {
                    let what = format!("declared in {list}");
                    self.found(root.then(key(list)).then(key(self.name)), true, what);
                }
            }
            if let Some(Value::Object(pointer)) = show.pointer("/input/pointer") {
                for (field, name) in pointer {
                    if name.as_str() == Some(self.name) {
                        let at = input.then(key("pointer")).then(key(field));
                        self.found(at, false, format!("pointer {field}"));
                    }
                }
            }
        }
        if let Some(Value::Object(values)) = show.get("values") {
            for (name, value) in values {
                let at = root.then(key("values")).then(key(name));
                self.timelines(value, &at, &format!("value {name}"));
            }
        }
        if let Some(layers) = show.get("layers") {
            self.layers(layers, &root.then(key("layers")), "");
        }
        if let Some(Value::Array(scenes)) = show.get("scenes") {
            for (i, scene) in scenes.iter().enumerate() {
                let at = root.then(key("scenes")).then(Part::Index(i));
                let name = scene.get("name").and_then(Value::as_str).unwrap_or("");
                if self.trigger {
                    self.triggers(
                        scene.get("trigger"),
                        at.then(key("trigger")),
                        &format!("scene {name}"),
                    );
                }
                if let Some(layers) = scene.get("layers") {
                    self.layers(
                        layers,
                        &at.then(key("layers")),
                        &format!(" in scene {name}"),
                    );
                }
            }
        }
    }

    fn layers(&mut self, layers: &Value, at: &Pointer, scene: &str) {
        let Value::Array(layers) = layers else {
            return;
        };
        for (i, layer) in layers.iter().enumerate() {
            let at = at.then(Part::Index(i));
            let name = layer
                .get("name")
                .or_else(|| layer.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let on = format!("layer {name}{scene}");
            self.timelines(layer, &at, &on);
            let kind = layer.get("type").and_then(Value::as_str).unwrap_or("layer");
            if self.trigger {
                for field in ["trigger", "stop", "on_end"] {
                    self.triggers(
                        layer.get(field),
                        at.then(key(field)),
                        &format!("{kind} {field} on {on}"),
                    );
                }
                self.triggers(
                    layer.get("press").and_then(|p| p.get("trigger")),
                    at.then(key("press")).then(key("trigger")),
                    &format!("press on {on}"),
                );
                self.triggers(
                    layer.pointer("/display/reel/spin"),
                    at.then(key("display")).then(key("reel")).then(key("spin")),
                    &format!("reel spin on {on}"),
                );
            } else {
                for field in ["when", "while"] {
                    self.reading(
                        layer.get(field),
                        at.then(key(field)),
                        format!("{kind} {field} on {on}"),
                    );
                }
                if let Some(Value::Array(bindings)) = layer.get("bindings") {
                    for (j, binding) in bindings.iter().enumerate() {
                        let property = binding
                            .get("property")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        self.reading(
                            Some(binding),
                            at.then(key("bindings")).then(Part::Index(j)),
                            format!("binding {property} on {on}"),
                        );
                    }
                }
            }
            for children in ["children", "parts"] {
                if let Some(inner) = layer.get(children) {
                    self.layers(inner, &at.then(key(children)), scene);
                }
            }
        }
    }

    /// The timelines of a layer or a value, `on` saying which.
    fn timelines(&mut self, owner: &Value, at: &Pointer, on: &str) {
        let Some(Value::Array(timelines)) = owner.get("timelines") else {
            return;
        };
        for (i, timeline) in timelines.iter().enumerate() {
            let at = at.then(key("timelines")).then(Part::Index(i));
            let name = timeline.get("name").and_then(Value::as_str).unwrap_or("");
            if self.trigger {
                self.triggers(
                    timeline.get("trigger"),
                    at.then(key("trigger")),
                    &format!("timeline {name} on {on}"),
                );
                self.triggers(
                    timeline.get("on_end"),
                    at.then(key("on_end")),
                    &format!("on_end of timeline {name} on {on}"),
                );
            } else {
                for field in ["when", "while"] {
                    self.reading(
                        timeline.get(field),
                        at.then(key(field)),
                        format!("{field} of timeline {name} on {on}"),
                    );
                }
            }
        }
    }

    fn driver(&mut self, driver: &Value) {
        let Some(Value::Array(steps)) = driver.get("steps") else {
            return;
        };
        for (i, step) in steps.iter().enumerate() {
            let at = Pointer::default().then(key("steps")).then(Part::Index(i));
            let what = format!("driver step {}", i + 1);
            if self.trigger {
                self.triggers(step.get("trigger"), at.then(key("trigger")), &what);
            } else if step.get("set").and_then(|s| s.get(self.name)).is_some() {
                let at = at.then(key("set")).then(key(self.name));
                self.found(at, true, format!("{what} sets it"));
            }
        }
    }
}

fn key(name: &str) -> Part {
    Part::Key(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHOW: &str = r##"{
  "format": 1, "name": "t", "size": [10, 10],
  "variables": {"score": 0, "mode": "a"},
  "values": {"page": {"timelines": [{"name": "p", "trigger": ["next", "skip"], "on_end": "next", "keys": []}]}},
  "input": {"keys": {"ArrowRight": "next", " ": "next", "ArrowLeft": "prev"}, "press": "next",
            "pointer": {"x": "score"}},
  "layers": [
    {"name": "next", "type": "group", "children": [
      {"name": "bell", "type": "sound", "sound": "bell", "trigger": "next", "stop": ["prev", "next"],
       "when": {"variable": "score", "threshold": 1}}
    ]}
  ],
  "scenes": [
    {"name": "a", "trigger": ["start", "next"], "layers": [
      {"name": "n", "type": "shape", "press": {"trigger": "next"},
       "bindings": [{"property": "opacity", "variable": "score", "map": {"next": 1}}],
       "timelines": [{"name": "next", "trigger": "next", "on_end": "prev", "tracks": []},
                     {"name": "lit", "while": {"variable": "score"}, "tracks": []}]}
    ]}
  ]
}"##;

    const DRIVER_TEXT: &str = "{\n  \"steps\": [\n    {\"trigger\": \"next\"},\n    {\"set\": {\"score\": 2}},\n    {\"wait\": 1}\n  ]\n}\n";

    fn whats(plan: &Plan) -> Vec<&str> {
        plan.uses.iter().map(|u| u.what.as_str()).collect()
    }

    #[test]
    fn a_trigger_is_renamed_wherever_it_is_used_as_one_step() {
        let mut document = Document::parse(SHOW).unwrap();
        let show = document.value();
        let driver: Value = serde_json::from_str(DRIVER_TEXT).unwrap();
        let plan = plan(Kind::Trigger, "next", "forward", &show, Some(&driver)).unwrap();
        assert_eq!(
            whats(&plan),
            [
                "key ArrowRight",
                "key Space",
                "a press on nothing pressable",
                "timeline p on value page",
                "on_end of timeline p on value page",
                "sound trigger on layer bell",
                "sound stop on layer bell",
                "scene a",
                "timeline next on layer n in scene a",
                "press on layer n in scene a",
                "driver step 1",
            ]
        );
        apply(&plan, &mut document, Some(DRIVER_TEXT)).unwrap();
        let after = document.value();
        assert_eq!(
            after["input"]["keys"],
            json!({"ArrowRight": "forward", " ": "forward", "ArrowLeft": "prev"})
        );
        assert_eq!(after["input"]["press"], "forward");
        assert_eq!(
            after["values"]["page"]["timelines"][0]["trigger"],
            json!(["forward", "skip"])
        );
        assert_eq!(after["values"]["page"]["timelines"][0]["on_end"], "forward");
        let bell = &after["layers"][0]["children"][0];
        assert_eq!(bell["trigger"], "forward");
        assert_eq!(bell["stop"], json!(["prev", "forward"]));
        let scene = &after["scenes"][0];
        assert_eq!(scene["trigger"], json!(["start", "forward"]));
        let n = &scene["layers"][0];
        assert_eq!(n["timelines"][0]["trigger"], "forward");
        assert_eq!(
            n["timelines"][0]["name"], "next",
            "a timeline's name is not a trigger"
        );
        assert_eq!(n["timelines"][0]["on_end"], "prev");
        assert_eq!(after["layers"][0]["name"], "next", "nor is a layer's");
        assert_eq!(
            n["bindings"][0]["map"],
            json!({"next": 1}),
            "nor what a map lists"
        );
        assert_eq!(n["press"]["trigger"], "forward");
        assert_eq!(
            document.file(DRIVER),
            Some(DRIVER_TEXT.replace("\"next\"", "\"forward\"").as_str()),
            "the driver keeps its layout"
        );
        assert_eq!(
            document.text(),
            SHOW.replace("\"next\"", "\"forward\"")
                .replace("\"name\": \"forward\"", "\"name\": \"next\"")
                .replace("{\"forward\": 1}", "{\"next\": 1}"),
            "the rest of the text as written"
        );
        assert_eq!(document.steps().len(), 1, "one step");
        assert!(document.undo());
        assert_eq!(document.text(), SHOW);
        assert_eq!(document.file(DRIVER), Some(DRIVER_TEXT));
        assert!(document.redo());
        assert!(document.file(DRIVER).unwrap().contains("forward"));
    }

    #[test]
    fn a_variable_is_renamed_in_its_readings_and_the_drivers_sets() {
        let mut document = Document::parse(SHOW).unwrap();
        let show = document.value();
        let driver: Value = serde_json::from_str(DRIVER_TEXT).unwrap();
        let plan = plan(Kind::Variable, "score", "points", &show, Some(&driver)).unwrap();
        assert_eq!(
            whats(&plan),
            [
                "declared in variables",
                "pointer x",
                "sound when on layer bell",
                "while of timeline lit on layer n in scene a",
                "binding opacity on layer n in scene a",
                "driver step 2 sets it",
            ]
        );
        apply(&plan, &mut document, Some(DRIVER_TEXT)).unwrap();
        let after = document.value();
        assert_eq!(after["variables"], json!({"points": 0, "mode": "a"}));
        assert!(
            document
                .text()
                .contains("\"variables\": {\"points\": 0, \"mode\": \"a\"}"),
            "the declaration stays where it was"
        );
        assert_eq!(after["input"]["pointer"]["x"], "points");
        assert_eq!(
            after["layers"][0]["children"][0]["when"]["variable"],
            "points"
        );
        let n = &after["scenes"][0]["layers"][0];
        assert_eq!(n["bindings"][0]["variable"], "points");
        assert_eq!(n["timelines"][1]["while"]["variable"], "points");
        assert!(
            document
                .file(DRIVER)
                .unwrap()
                .contains("{\"set\": {\"points\": 2}}")
        );
        assert_eq!(document.steps().len(), 1);
        assert!(document.undo());
        assert_eq!(document.text(), SHOW);
    }

    #[test]
    fn a_name_in_use_or_a_name_nothing_uses_is_refused() {
        let show: Value = serde_json::from_str(SHOW).unwrap();
        assert!(plan(Kind::Trigger, "next", "prev", &show, None).is_err());
        assert!(plan(Kind::Trigger, "next", "start", &show, None).is_err());
        assert!(plan(Kind::Variable, "score", "mode", &show, None).is_err());
        assert!(
            plan(Kind::Variable, "score", "page", &show, None).is_err(),
            "a value"
        );
        assert!(plan(Kind::Value, "page", "score", &show, None).is_err());
        assert!(plan(Kind::Trigger, "nothing", "else", &show, None).is_err());
        assert!(plan(Kind::Trigger, "next", " next2", &show, None).is_err());
        assert!(plan(Kind::Trigger, "next", "next", &show, None).is_err());
        let value = plan(Kind::Value, "page", "slide", &show, None).unwrap();
        assert_eq!(whats(&value), ["declared in values"]);
    }

    #[test]
    fn a_rename_that_fails_part_way_changes_nothing() {
        let mut document = Document::parse(SHOW).unwrap();
        let show = document.value();
        let mut plan = plan(Kind::Trigger, "next", "forward", &show, None).unwrap();
        plan.uses.push(Use {
            file: "show.json",
            at: Pointer::parse("/nowhere/0").unwrap(),
            key: false,
            what: "nowhere".to_owned(),
        });
        assert!(apply(&plan, &mut document, None).is_err());
        assert_eq!(document.text(), SHOW);
        assert!(!document.can_undo() && !document.can_redo());
    }
}
