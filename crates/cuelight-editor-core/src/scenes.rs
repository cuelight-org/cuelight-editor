//! Making and arranging scenes in the document: a new scene, and
//! deleting, reordering and renaming one, each as one undo step; the
//! triggers that enter a scene; and what entering it does, from the
//! show as the engine reads it.
//!
//! Nothing in the format refers to a scene by its name: triggers enter
//! scenes. A rename changes the name alone, and is refused for a name
//! another scene has, which would leave two scenes the editor and the
//! log could not tell apart.

use cuelight_core::{Layer, Show, Timeline};
use serde_json::{Value, json};

use crate::document::{Document, Part, Pointer};
use crate::layers::{said, step, unique_name};

/// The show's list of scenes.
fn list() -> Pointer {
    Pointer(vec![Part::Key("scenes".to_owned())])
}

/// The scene at `index` in the document.
pub fn pointer(index: usize) -> Pointer {
    list().then(Part::Index(index))
}

/// The scenes' names, in order.
pub fn names(document: &Document) -> Vec<String> {
    let Some(Value::Array(scenes)) = document.get(&list()).map(|n| n.value()) else {
        return Vec::new();
    };
    scenes
        .iter()
        .map(|scene| {
            scene
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// Put a new scene at `index` (past the end appends): `scene`, `scene_2`,
/// ..., entered by a trigger of its own name and with no layers yet. The
/// show's list of scenes is made when it has none. Gives its index.
pub fn add(document: &mut Document, index: usize) -> Result<usize, String> {
    let taken = names(document);
    let name = unique_name(&taken, "scene");
    let index = index.min(taken.len());
    step(document, |document| {
        if document.get(&list()).is_none() {
            document.insert(&list(), json!([])).map_err(said)?;
        }
        let scene = json!({ "name": name, "trigger": name, "layers": [] });
        document.insert_item(&pointer(index), scene).map_err(said)?;
        Ok(index)
    })
}

/// Take the scene at `index` out, its layers with it. A show left
/// without scenes loses its empty list too.
pub fn delete(document: &mut Document, index: usize) -> Result<(), String> {
    let count = names(document).len();
    if index >= count {
        return Err("no such scene".to_owned());
    }
    step(document, |document| {
        if count == 1 {
            document.remove(&list()).map_err(said)
        } else {
            document.remove(&pointer(index)).map_err(said)
        }
    })
}

/// Move the scene at `index` one place earlier (`up`) or later. The
/// first scene is the one a show starts in. Gives where it is now.
pub fn reorder(document: &mut Document, index: usize, up: bool) -> Result<usize, String> {
    let count = names(document).len();
    if index >= count {
        return Err("no such scene".to_owned());
    }
    let to = match up {
        true if index == 0 => return Err("already first".to_owned()),
        false if index + 1 >= count => return Err("already last".to_owned()),
        true => index - 1,
        false => index + 1,
    };
    step(document, |document| {
        document
            .move_item(&pointer(index), &pointer(to))
            .map_err(said)?;
        Ok(to)
    })
}

/// Name the scene at `index` `to`, refused for an empty name or one
/// another scene has.
pub fn rename(document: &mut Document, index: usize, to: &str) -> Result<(), String> {
    let to = to.trim();
    if to.is_empty() {
        return Err("a scene needs a name".to_owned());
    }
    let names = names(document);
    if names.get(index).is_none() {
        return Err("no such scene".to_owned());
    }
    if names
        .iter()
        .enumerate()
        .any(|(i, name)| i != index && name == to)
    {
        return Err(format!("another scene is called {to}"));
    }
    let at = pointer(index).then(Part::Key("name".to_owned()));
    step(document, |document| {
        document.set(&at, Value::from(to)).map_err(said)
    })
}

/// The triggers that enter the scene at `index`, as written: one name or
/// a list of them.
pub fn triggers(document: &Document, index: usize) -> Vec<String> {
    let at = pointer(index).then(Part::Key("trigger".to_owned()));
    match document.get(&at).map(|n| n.value()) {
        Some(Value::String(name)) => vec![name],
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(|n| n.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// Write the triggers that enter the scene at `index`: none takes the
/// key out, one is written as a name, more as a list. Blank names and
/// repeats are left out.
pub fn set_triggers(document: &mut Document, index: usize, names: &[String]) -> Result<(), String> {
    if document.get(&pointer(index)).is_none() {
        return Err("no such scene".to_owned());
    }
    let mut kept: Vec<&str> = Vec::new();
    for name in names.iter().map(|n| n.trim()) {
        if !name.is_empty() && !kept.contains(&name) {
            kept.push(name);
        }
    }
    let at = pointer(index).then(Part::Key("trigger".to_owned()));
    let value = match kept.as_slice() {
        [] => None,
        [one] => Some(Value::from(*one)),
        many => Some(Value::from(many.to_vec())),
    };
    let written = document.get(&at).map(|n| n.value());
    if written == value {
        return Ok(());
    }
    step(document, |document| match (written, value) {
        (Some(_), None) => document.remove(&at).map_err(said),
        (Some(_), Some(value)) => document.set(&at, value).map_err(said),
        (None, Some(value)) => document.insert(&at, value).map_err(said),
        (None, None) => Ok(()),
    })
}

/// What entering a scene does, timeline by timeline, each named
/// `layer: timeline` (`group/layer: timeline` inside a group).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entering {
    /// The scene's `autoplay` timelines, started again from 0.
    pub restarts: Vec<String>,
    /// The scene's `while` timelines, started again where their
    /// condition holds.
    pub whiles: Vec<String>,
    /// The scene's `when` timelines: not replayed, unless their
    /// condition turned true while the scene was away.
    pub whens: Vec<String>,
    /// Timelines in the show's layers or the scene's that declare a
    /// trigger entering it, started by that trigger: `trigger: layer:
    /// timeline`.
    pub triggered: Vec<String>,
}

/// What entering the scene at `index` of `show` does.
pub fn entering(show: &Show, index: usize) -> Entering {
    let mut out = Entering::default();
    let Some(scene) = show.scenes.get(index) else {
        return out;
    };
    each_timeline(&scene.layers, "", &mut |name, timeline| {
        if timeline.whilst.is_some() {
            out.whiles.push(name.clone());
        } else if timeline.autoplay {
            out.restarts.push(name.clone());
        }
        if timeline.when.is_some() {
            out.whens.push(name);
        }
    });
    for layers in [&show.layers, &scene.layers] {
        each_timeline(layers, "", &mut |name, timeline| {
            if let Some(trigger) = scene.trigger.iter().find(|t| timeline.trigger.contains(t)) {
                out.triggered.push(format!("{trigger}: {name}"));
            }
        });
    }
    out
}

/// Every timeline of `layers` and their children, with its name.
fn each_timeline(layers: &[Layer], above: &str, visit: &mut impl FnMut(String, &Timeline)) {
    for layer in layers {
        let at = if above.is_empty() {
            layer.name.clone()
        } else {
            format!("{above}/{}", layer.name)
        };
        for timeline in &layer.timelines {
            visit(format!("{at}: {}", timeline.name), timeline);
        }
        each_timeline(layer.children(), &at, visit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: &str = r#"{
  "format": 1,
  "name": "t",
  "size": [10, 10],
  "layers": [],
  "scenes": [
    { "name": "a", "trigger": "a", "layers": [] },
    { "name": "b", "trigger": ["b", "b_back"], "layers": [] }
  ]
}
"#;

    fn document() -> Document {
        Document::parse(SHOW).unwrap()
    }

    #[test]
    fn a_new_scene_is_named_apart_and_entered_by_its_own_trigger() {
        let mut document = document();
        assert_eq!(add(&mut document, 1), Ok(1));
        assert_eq!(names(&document), ["a", "scene", "b"]);
        assert_eq!(triggers(&document, 1), ["scene"]);
        assert_eq!(add(&mut document, usize::MAX), Ok(3));
        assert_eq!(names(&document), ["a", "scene", "b", "scene_2"]);
        assert!(document.undo());
        assert!(document.undo());
        assert_eq!(document.text(), SHOW);
    }

    #[test]
    fn a_show_without_scenes_gets_its_list_and_loses_it_again() {
        let text =
            "{\n  \"format\": 1,\n  \"name\": \"t\",\n  \"size\": [10, 10],\n  \"layers\": []\n}\n";
        let mut document = Document::parse(text).unwrap();
        assert_eq!(add(&mut document, 0), Ok(0));
        assert_eq!(names(&document), ["scene"]);
        let value = document.value();
        let _: Show = serde_json::from_value(value).unwrap();
        delete(&mut document, 0).unwrap();
        assert!(document.value().get("scenes").is_none());
        assert!(document.undo());
        assert!(document.undo());
        assert_eq!(document.text(), text);
    }

    #[test]
    fn scenes_are_deleted_reordered_and_renamed_in_one_step_each() {
        let mut document = document();
        assert_eq!(
            reorder(&mut document, 0, true),
            Err("already first".to_owned())
        );
        assert_eq!(
            reorder(&mut document, 1, false),
            Err("already last".to_owned())
        );
        assert_eq!(reorder(&mut document, 1, true), Ok(0));
        assert_eq!(names(&document), ["b", "a"]);
        // The scene moves as written, its triggers' list on one line.
        assert!(
            document
                .text()
                .contains(r#"{ "name": "b", "trigger": ["b", "b_back"], "layers": [] },"#)
        );
        assert!(document.undo());
        assert_eq!(document.text(), SHOW);

        assert_eq!(
            rename(&mut document, 0, "b"),
            Err("another scene is called b".to_owned())
        );
        assert_eq!(
            rename(&mut document, 0, " "),
            Err("a scene needs a name".to_owned())
        );
        rename(&mut document, 0, "intro").unwrap();
        assert_eq!(names(&document), ["intro", "b"]);
        assert_eq!(triggers(&document, 0), ["a"], "triggers stay as they were");

        delete(&mut document, 0).unwrap();
        assert_eq!(names(&document), ["b"]);
        assert!(document.undo());
        assert!(document.undo());
        assert_eq!(document.text(), SHOW);
    }

    #[test]
    fn triggers_are_written_as_a_name_a_list_or_not_at_all() {
        let mut document = document();
        set_triggers(&mut document, 1, &["b".to_owned()]).unwrap();
        assert_eq!(document.value()["scenes"][1]["trigger"], "b");
        set_triggers(
            &mut document,
            0,
            &["a".into(), " ".into(), "start".into(), "a".into()],
        )
        .unwrap();
        assert_eq!(
            document.value()["scenes"][0]["trigger"],
            json!(["a", "start"])
        );
        set_triggers(&mut document, 0, &[]).unwrap();
        assert!(document.value()["scenes"][0].get("trigger").is_none());
        set_triggers(&mut document, 0, &["go".into()]).unwrap();
        assert_eq!(triggers(&document, 0), ["go"]);
        let steps = document.steps().len();
        set_triggers(&mut document, 0, &["go".into()]).unwrap();
        assert_eq!(
            document.steps().len(),
            steps,
            "nothing changed, nothing to undo"
        );
        for _ in 0..4 {
            assert!(document.undo());
        }
        assert_eq!(document.text(), SHOW);
    }

    #[test]
    fn entering_says_which_timelines_start_again_and_which_wait() {
        let show: Show = serde_json::from_value(json!({
            "format": 1, "name": "t", "size": [10, 10],
            "layers": [
                { "name": "hud", "type": "group", "children": [
                    { "name": "flash", "type": "shape", "shape": { "rect": [0, 0, 1, 1] }, "fill": "#FFFFFF",
                      "timelines": [{ "name": "in", "trigger": "go", "tracks": [] }] }
                ] }
            ],
            "scenes": [
                { "name": "a", "trigger": "go", "layers": [
                    { "name": "dot", "type": "shape", "shape": { "rect": [0, 0, 1, 1] }, "fill": "#FFFFFF",
                      "timelines": [
                        { "name": "intro", "autoplay": true, "tracks": [] },
                        { "name": "blink", "loop": true, "while": { "variable": "lit" }, "tracks": [] },
                        { "name": "pop", "when": { "variable": "score" }, "tracks": [] }
                      ] }
                ] }
            ]
        }))
        .unwrap();
        assert_eq!(
            entering(&show, 0),
            Entering {
                restarts: vec!["dot: intro".to_owned()],
                whiles: vec!["dot: blink".to_owned()],
                whens: vec!["dot: pop".to_owned()],
                triggered: vec!["go: hud/flash: in".to_owned()],
            }
        );
        assert_eq!(entering(&show, 1), Entering::default());
    }
}
