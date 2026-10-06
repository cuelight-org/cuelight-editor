//! Changing a layer's property in the document: where the JSON writes a
//! property, reading what was typed for it, and setting it whether the
//! layer writes it already or leaves it at its default.

use cuelight_core::Property;
use serde_json::Value;

use crate::document::{Document, EditError, Part, Pointer};

/// How a property is typed in the inspector, for the properties that
/// can be edited there so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Number,
    Text,
}

/// How `property` is typed, or `None` while the inspector cannot edit
/// it yet.
pub fn input(property: Property) -> Option<Input> {
    match property {
        Property::X
        | Property::Y
        | Property::Rotation
        | Property::Scale
        | Property::ScaleX
        | Property::ScaleY
        | Property::Opacity
        | Property::Reveal
        | Property::Frame
        | Property::Gain => Some(Input::Number),
        Property::Text => Some(Input::Text),
        _ => None,
    }
}

/// Where a layer writes `property`, under the layer at `layer`: a key of
/// the layer's own object, named as the format names the property.
pub fn pointer(layer: &Pointer, property: Property) -> Option<Pointer> {
    input(property)?;
    let name = serde_json::to_value(property).ok()?.as_str()?.to_owned();
    Some(layer.then(Part::Key(name)))
}

/// What was typed, as the value the document gets: a number written as
/// an integer when it is one, so `40` stays `40` and not `40.0`.
pub fn parse(property: Property, typed: &str) -> Result<Value, String> {
    match input(property) {
        Some(Input::Number) => {
            let n: f64 = typed
                .trim()
                .parse()
                .map_err(|_| format!("{typed:?} is not a number"))?;
            if !n.is_finite() {
                return Err(format!("{typed:?} is not a number"));
            }
            Ok(if n.fract() == 0.0 && n.abs() < 1e15 {
                Value::from(n as i64)
            } else {
                Value::from(n)
            })
        }
        Some(Input::Text) => Ok(Value::String(typed.to_owned())),
        None => Err("this property cannot be edited here yet".to_owned()),
    }
}

/// Set `property` of the layer at `layer` to `value`: replace it where
/// the layer writes it, or add it after the layer's other keys.
pub fn set(
    document: &mut Document,
    layer: &Pointer,
    property: Property,
    value: Value,
) -> Result<(), EditError> {
    let Some(at) = pointer(layer, property) else {
        return Err(EditError::NotFound(layer.clone()));
    };
    if document.get(&at).is_some() {
        document.set(&at, value)
    } else {
        document.insert(&at, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHOW: &str = r#"{
  "layers": [
    {"name": "dot", "type": "shape", "x": 10, "shape": {"circle": [0, 0, 4]}}
  ]
}
"#;

    fn dot() -> Pointer {
        Pointer::parse("/layers/0").unwrap()
    }

    #[test]
    fn a_written_property_changes_in_place() {
        let mut document = Document::parse(SHOW).unwrap();
        set(&mut document, &dot(), Property::X, json!(12)).unwrap();
        assert_eq!(document.text(), SHOW.replace("\"x\": 10", "\"x\": 12"));
    }

    #[test]
    fn an_unwritten_property_is_added_after_the_others() {
        let mut document = Document::parse(SHOW).unwrap();
        set(&mut document, &dot(), Property::Opacity, json!(0.5)).unwrap();
        assert_eq!(document.value()["layers"][0]["opacity"], json!(0.5));
        let keys: Vec<_> = document.value()["layers"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["name", "type", "x", "shape", "opacity"]);
        assert!(document.undo());
        assert_eq!(document.text(), SHOW);
    }

    #[test]
    fn typed_numbers_keep_integers_whole() {
        assert_eq!(parse(Property::X, "40"), Ok(json!(40)));
        assert_eq!(parse(Property::Opacity, " 0.25 "), Ok(json!(0.25)));
        assert!(parse(Property::X, "forty").is_err());
        assert!(parse(Property::X, "inf").is_err());
        assert_eq!(parse(Property::Text, "HELLO"), Ok(json!("HELLO")));
        assert!(parse(Property::Tint, "#FFF").is_err(), "not editable yet");
    }
}
