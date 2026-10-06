//! Changing a layer's property in the document: where the JSON writes a
//! property, reading what was typed for it, and setting it whether the
//! layer writes it already or leaves it at its default.

use cuelight_core::Property;
use serde_json::Value;

use crate::document::{Document, EditError, Part, Pointer};

/// How a property is typed in the inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Number,
    Text,
    /// On or off.
    Toggle,
    /// `#RRGGBB` or `#RRGGBBAA`, or empty for none.
    Colour,
    /// One of the names the show has: a font style, a sound, a clip.
    Choice,
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
        | Property::Gain
        | Property::TileX
        | Property::TileY => Some(Input::Number),
        Property::Text => Some(Input::Text),
        Property::Visible => Some(Input::Toggle),
        Property::Tint => Some(Input::Colour),
        Property::Font | Property::Sound | Property::Video => Some(Input::Choice),
    }
}

/// Where a layer writes `property`, under the layer at `layer`: a key of
/// the layer's own object, named as the format names the property, but
/// for a tiled image's pattern offset, which is `repeat.offset`.
pub fn pointer(layer: &Pointer, property: Property) -> Option<Pointer> {
    input(property)?;
    let offset = |i| {
        layer
            .then(Part::Key("repeat".to_owned()))
            .then(Part::Key("offset".to_owned()))
            .then(Part::Index(i))
    };
    match property {
        Property::TileX => Some(offset(0)),
        Property::TileY => Some(offset(1)),
        _ => {
            let name = serde_json::to_value(property).ok()?.as_str()?.to_owned();
            Some(layer.then(Part::Key(name)))
        }
    }
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
        Some(Input::Text | Input::Choice) => Ok(Value::String(typed.to_owned())),
        Some(Input::Toggle) => match typed.trim() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(format!("{typed:?} is not true or false")),
        },
        Some(Input::Colour) => {
            let colour = typed.trim();
            let digits = colour.strip_prefix('#').unwrap_or("x");
            if colour.is_empty()
                || ((digits.len() == 6 || digits.len() == 8)
                    && digits.chars().all(|c| c.is_ascii_hexdigit()))
            {
                Ok(Value::String(colour.to_uppercase()))
            } else {
                Err(format!("{typed:?} is not a colour: #RRGGBB or #RRGGBBAA"))
            }
        }
        None => Err("this property cannot be edited here".to_owned()),
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
        return document.set(&at, value);
    }
    // A pattern offset the layer does not write yet: the pair, with the
    // other coordinate at 0. A layer that does not repeat has none.
    let zero = || Value::from(0);
    let pair = match property {
        Property::TileX => Some(vec![value.clone(), zero()]),
        Property::TileY => Some(vec![zero(), value.clone()]),
        _ => None,
    };
    if let Some(pair) = pair {
        let repeat = layer.then(Part::Key("repeat".to_owned()));
        if document.get(&repeat).is_none() {
            return Err(EditError::NotFound(repeat));
        }
        return document.insert(
            &repeat.then(Part::Key("offset".to_owned())),
            Value::Array(pair),
        );
    }
    document.insert(&at, value)
}

/// Whether `value` is what the layer `layer` (its JSON) has for
/// `property` when it does not write it: the engine is asked, with the
/// key taken out. A key the layer cannot do without has no default.
pub fn is_default(layer: &Value, property: Property, value: &Value) -> bool {
    if matches!(property, Property::TileX | Property::TileY) {
        return value.as_f64() == Some(0.0);
    }
    let Some(name) = serde_json::to_value(property)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
    else {
        return false;
    };
    let mut without = layer.clone();
    if let Some(object) = without.as_object_mut() {
        object.remove(&name);
    }
    let Ok(read) = serde_json::from_value::<cuelight_core::Layer>(without) else {
        return false;
    };
    let default = match read.base_value(property) {
        Some(cuelight_core::Value::Number(n)) => Value::from(n),
        Some(cuelight_core::Value::Bool(b)) => Value::Bool(b),
        Some(cuelight_core::Value::Text(t)) => Value::String(t),
        _ => return false,
    };
    same(&default, value)
}

/// Two JSON values that mean the same: `1` and `1.0`, `#ffb000` and
/// `#FFB000`.
pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::String(x), Value::String(y)) => x.eq_ignore_ascii_case(y),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        _ => a == b,
    }
}

/// Whether the layer at `layer` writes `property`, rather than leaving
/// it to its default.
pub fn written(document: &Document, layer: &Pointer, property: Property) -> bool {
    pointer(layer, property).is_some_and(|at| document.get(&at).is_some())
}

/// Take `property` out of the layer at `layer`, back to its default. A
/// pattern offset goes back to 0, as the pair it sits in has two.
pub fn unset(
    document: &mut Document,
    layer: &Pointer,
    property: Property,
) -> Result<(), EditError> {
    let Some(at) = pointer(layer, property) else {
        return Err(EditError::NotFound(layer.clone()));
    };
    match property {
        Property::TileX | Property::TileY => document.set(&at, Value::from(0)),
        _ => document.remove(&at),
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
    fn a_property_unset_goes_back_to_its_default() {
        let mut document = Document::parse(SHOW).unwrap();
        assert!(written(&document, &dot(), Property::X));
        assert!(!written(&document, &dot(), Property::Opacity));
        unset(&mut document, &dot(), Property::X).unwrap();
        assert!(!written(&document, &dot(), Property::X));
        assert!(document.undo());
        assert_eq!(document.text(), SHOW);
    }

    #[test]
    fn a_default_is_what_the_engine_reads_without_the_key() {
        let dot = json!({"name": "dot", "type": "shape", "x": 10, "opacity": 0.5,
                         "shape": {"circle": [0, 0, 4]}, "fill": "#FFFFFF"});
        assert!(is_default(&dot, Property::Opacity, &json!(1)));
        assert!(is_default(&dot, Property::Opacity, &json!(1.0)));
        assert!(!is_default(&dot, Property::Opacity, &json!(0.5)));
        assert!(is_default(&dot, Property::X, &json!(0)));
        assert!(is_default(&dot, Property::Visible, &json!(true)));
        assert!(is_default(&dot, Property::Scale, &json!(1)));
    }

    #[test]
    fn typed_numbers_keep_integers_whole() {
        assert_eq!(parse(Property::X, "40"), Ok(json!(40)));
        assert_eq!(parse(Property::Opacity, " 0.25 "), Ok(json!(0.25)));
        assert!(parse(Property::X, "forty").is_err());
        assert!(parse(Property::X, "inf").is_err());
        assert_eq!(parse(Property::Text, "HELLO"), Ok(json!("HELLO")));
    }

    #[test]
    fn colours_toggles_and_choices_are_read() {
        assert_eq!(parse(Property::Tint, "#ffb000"), Ok(json!("#FFB000")));
        assert_eq!(parse(Property::Tint, "#FFB00080"), Ok(json!("#FFB00080")));
        assert_eq!(parse(Property::Tint, ""), Ok(json!("")), "no tint");
        assert!(parse(Property::Tint, "#FFF").is_err());
        assert!(parse(Property::Tint, "orange").is_err());
        assert_eq!(parse(Property::Visible, "false"), Ok(json!(false)));
        assert!(parse(Property::Visible, "no").is_err());
        assert_eq!(parse(Property::Font, "title"), Ok(json!("title")));
    }

    #[test]
    fn a_pattern_offset_goes_into_repeat() {
        let tiled = r#"{"layers": [{"name": "floor", "type": "image", "image": "tile", "repeat": {"size": [8, 8]}}]}"#;
        let mut document = Document::parse(tiled).unwrap();
        set(&mut document, &dot(), Property::TileY, json!(3)).unwrap();
        assert_eq!(
            document.value()["layers"][0]["repeat"]["offset"],
            json!([0, 3])
        );
        set(&mut document, &dot(), Property::TileX, json!(5)).unwrap();
        assert_eq!(
            document.value()["layers"][0]["repeat"]["offset"],
            json!([5, 3])
        );

        let mut plain = Document::parse(SHOW).unwrap();
        assert!(
            set(&mut plain, &dot(), Property::TileX, json!(1)).is_err(),
            "no repeat"
        );
    }
}
