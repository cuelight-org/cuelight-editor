//! What a host can say to a show: the triggers it listens to, the
//! variables it reads, the keys and presses it names, and the values of
//! its own a host can take over. Read from the show, so the panel that
//! offers them is right for any show.

use std::collections::{BTreeMap, BTreeSet};

use cuelight_core::{Layer, Show, Value};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inputs {
    /// Every trigger something in the show answers to, and every trigger
    /// a key or a press fires.
    pub triggers: BTreeSet<String>,
    /// Where each trigger is listened to, which is how a panel groups
    /// them: the ones that open a scene, the ones the show hears
    /// anywhere, and the ones only one scene hears.
    pub places: BTreeMap<String, Place>,
    /// The declared variables with their initial values.
    pub variables: BTreeMap<String, Value>,
    /// The values the show animates itself. Setting a variable of the
    /// same name takes one over.
    pub values: BTreeSet<String>,
    /// Keys the show names, with the trigger each fires.
    pub keys: BTreeMap<String, String>,
    /// Layers that can be pressed, by name, with the trigger each fires.
    pub pressable: Vec<(String, String)>,
    /// The trigger a press on nothing pressable fires.
    pub press_anywhere: Option<String>,
}

/// Where a trigger is listened to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    /// Firing it enters this scene.
    Opens(String),
    /// The show's own layers hear it, or more than one scene does.
    Anywhere,
    /// Only this scene hears it.
    Scene(String),
}

impl Inputs {
    pub fn of(show: &Show) -> Self {
        let mut triggers = show.triggers();
        triggers.extend(show.input.keys.values().cloned());
        triggers.extend(show.input.press.iter().cloned());
        let mut pressable = Vec::new();
        for layers in show.layer_trees() {
            presses(layers, &mut pressable);
        }
        triggers.extend(pressable.iter().map(|(_, trigger)| trigger.clone()));
        let places = places(show, &triggers);
        Self {
            triggers,
            places,
            variables: show.variables.clone(),
            values: show.values.keys().cloned().collect(),
            keys: show.input.keys.clone(),
            pressable,
            press_anywhere: show.input.press.clone(),
        }
    }
}

/// Where each trigger is heard, as the show says it.
fn places(show: &Show, triggers: &BTreeSet<String>) -> BTreeMap<String, Place> {
    use cuelight_core::Listened;
    let listened = show.listeners();
    triggers
        .iter()
        .map(|trigger| {
            let place = match listened.get(trigger) {
                Some(Listened::Opens(scene)) => Place::Opens(scene.clone()),
                Some(Listened::Scene(scene)) => Place::Scene(scene.clone()),
                Some(Listened::Anywhere) | None => Place::Anywhere,
            };
            (trigger.clone(), place)
        })
        .collect()
}

fn presses(layers: &[Layer], out: &mut Vec<(String, String)>) {
    for layer in layers {
        if let Some(press) = &layer.press {
            out.push((layer.name.clone(), press.trigger.clone()));
        }
        presses(layer.children(), out);
    }
}

/// A value typed into a field, read the way the show's own document
/// would read it: `true` and `false` are booleans, a number is a number,
/// anything else is text.
pub fn parse_value(text: &str) -> Value {
    match text.trim() {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        other => other
            .parse::<f64>()
            .map(Value::Number)
            .unwrap_or_else(|_| Value::Text(other.to_owned())),
    }
}

/// A value as a field shows it.
pub fn show_value(value: &Value) -> String {
    match value {
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Value::Text(t) => t.clone(),
        _ => format!("{value:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_get_the_place_they_are_heard() {
        let show: Show = serde_json::from_str(
            r##"{ "format": 1, "name": "t", "size": [10, 10],
                 "input": { "keys": { "ArrowRight": "next" } },
                 "layers": [
                   { "name": "bell", "type": "shape", "shape": { "rect": [0, 0, 1, 1] }, "fill": "#FFFFFF",
                     "timelines": [{ "name": "ring", "trigger": "ring", "tracks": [] }] }
                 ],
                 "scenes": [
                   { "name": "a", "trigger": "go_a", "layers": [
                     { "name": "n", "type": "shape", "shape": { "rect": [0, 0, 1, 1] }, "fill": "#FFFFFF",
                       "timelines": [{ "name": "next", "trigger": "next", "on_end": "go_b", "tracks": [] },
                                     { "name": "only", "trigger": "only_a", "tracks": [] }] } ] },
                   { "name": "b", "trigger": "go_b", "layers": [
                     { "name": "n", "type": "shape", "shape": { "rect": [0, 0, 1, 1] }, "fill": "#FFFFFF",
                       "timelines": [{ "name": "next", "trigger": "next", "tracks": [] }] } ] }
                 ] }"##,
        )
        .unwrap();
        let inputs = Inputs::of(&show);
        assert_eq!(inputs.places["go_a"], Place::Opens("a".into()));
        assert_eq!(
            inputs.places["ring"],
            Place::Anywhere,
            "the show's own layers hear it"
        );
        assert_eq!(inputs.places["next"], Place::Anywhere, "two scenes hear it");
        assert_eq!(inputs.places["only_a"], Place::Scene("a".into()));
    }
}
