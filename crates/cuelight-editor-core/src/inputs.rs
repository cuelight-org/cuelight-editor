//! What a host can say to a show: the triggers it listens to, the
//! variables it reads, the keys and presses it names, and the values of
//! its own a host can take over. Read from the show, so the panel that
//! offers them is right for any show.

use std::collections::{BTreeMap, BTreeSet};

use cuelight::{Layer, LayerKind, Show, Value};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inputs {
    /// Every trigger something in the show answers to, and every trigger
    /// a key or a press fires.
    pub triggers: BTreeSet<String>,
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
        Self {
            triggers,
            variables: show.variables.clone(),
            values: show.values.keys().cloned().collect(),
            keys: show.input.keys.clone(),
            pressable,
            press_anywhere: show.input.press.clone(),
        }
    }
}

fn presses(layers: &[Layer], out: &mut Vec<(String, String)>) {
    for layer in layers {
        if let Some(press) = &layer.press {
            out.push((layer.name.clone(), press.trigger.clone()));
        }
        if let LayerKind::Group { children, .. } = &layer.kind {
            presses(children, out);
        }
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
