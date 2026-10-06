//! A layer's fields that are not animatable properties: where it is
//! anchored, how it blends, a text's alignment, a shape's fill, a
//! sound's playback. The inspector edits them under the properties;
//! what they are, which kinds of layer have them and how each is typed
//! is said once, here.

use serde_json::Value;

use crate::document::{Document, EditError, Part, Pointer};

/// How a field is typed in the inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// One of these words.
    Choice(&'static [&'static str]),
    Toggle,
    Number,
    /// A whole number.
    Count,
    /// Two numbers, typed `40, 30`.
    Pair,
    /// `#RRGGBB` or `#RRGGBBAA`.
    Colour,
    Text,
    /// The name of one of the show's images or vector artwork.
    Artwork,
}

/// A field of a layer: where the layer writes it, how it is typed, and
/// which kinds of layer have it (empty for every kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// Its name in the inspector.
    pub label: &'static str,
    /// The keys down to it from the layer's object: `["stroke", "width"]`.
    pub path: &'static [&'static str],
    pub input: Input,
    pub kinds: &'static [&'static str],
    /// Kinds that have the key in the format but not the field: what has
    /// no content box cannot be anchored, a sound draws nothing to blend,
    /// overflow or press, and an artwork part has only its pivot and the
    /// animatable properties.
    pub not: &'static [&'static str],
    /// The object the field sits in needs it: taking the field out takes
    /// the object out too (a stroke without its colour, a press without
    /// its trigger).
    pub needed: bool,
}

const PLACES: &[&str] = &[
    "top_left",
    "top",
    "top_right",
    "left",
    "center",
    "right",
    "bottom_left",
    "bottom",
    "bottom_right",
];
const MEDIA: &[&str] = &["audio", "video"];

/// Every field the inspector edits, in the order it lists them.
pub const FIELDS: &[Field] = &[
    Field {
        label: "anchor",
        path: &["anchor"],
        input: Input::Choice(PLACES),
        kinds: &[],
        not: &["audio", "group", "part"],
        needed: false,
    },
    Field {
        label: "blend",
        path: &["blend"],
        input: Input::Choice(&["normal", "add", "screen", "multiply"]),
        kinds: &[],
        not: &["audio", "part"],
        needed: false,
    },
    Field {
        label: "overflow",
        path: &["overflow"],
        input: Input::Toggle,
        kinds: &[],
        not: &["audio", "part"],
        needed: false,
    },
    Field {
        label: "press",
        path: &["press", "trigger"],
        input: Input::Text,
        kinds: &[],
        not: &["audio", "part"],
        needed: true,
    },
    Field {
        label: "size",
        path: &["size"],
        input: Input::Pair,
        kinds: &["text", "digits", "image", "vector", "video"],
        not: &[],
        needed: false,
    },
    Field {
        label: "align",
        path: &["align"],
        input: Input::Choice(PLACES),
        kinds: &["text"],
        not: &[],
        needed: false,
    },
    Field {
        label: "digits",
        path: &["digits"],
        input: Input::Count,
        kinds: &["digits"],
        not: &[],
        needed: false,
    },
    Field {
        label: "justify",
        path: &["justify"],
        input: Input::Choice(&["left", "right"]),
        kinds: &["digits"],
        not: &[],
        needed: false,
    },
    Field {
        label: "image",
        path: &["image"],
        input: Input::Artwork,
        kinds: &["image"],
        not: &[],
        needed: false,
    },
    Field {
        label: "vector",
        path: &["vector"],
        input: Input::Artwork,
        kinds: &["vector"],
        not: &[],
        needed: false,
    },
    Field {
        label: "sampling",
        path: &["sampling"],
        input: Input::Choice(&["smooth", "nearest"]),
        kinds: &["image", "vector"],
        not: &[],
        needed: false,
    },
    Field {
        label: "fill",
        path: &["fill"],
        input: Input::Colour,
        kinds: &["shape"],
        not: &[],
        needed: false,
    },
    Field {
        label: "stroke",
        path: &["stroke", "color"],
        input: Input::Colour,
        kinds: &["shape"],
        not: &[],
        needed: true,
    },
    Field {
        label: "stroke width",
        path: &["stroke", "width"],
        input: Input::Number,
        kinds: &["shape"],
        not: &[],
        needed: false,
    },
    Field {
        label: "pivot",
        path: &["pivot"],
        input: Input::Pair,
        kinds: &["part"],
        not: &[],
        needed: false,
    },
    Field {
        label: "trigger",
        path: &["trigger"],
        input: Input::Text,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "stop",
        path: &["stop"],
        input: Input::Text,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "on_end",
        path: &["on_end"],
        input: Input::Text,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "autoplay",
        path: &["autoplay"],
        input: Input::Toggle,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "loop",
        path: &["loop"],
        input: Input::Toggle,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "delay",
        path: &["delay"],
        input: Input::Number,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "repeat",
        path: &["repeat"],
        input: Input::Count,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "rest",
        path: &["rest"],
        input: Input::Number,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "retrigger",
        path: &["retrigger"],
        input: Input::Choice(&["restart", "overlap", "ignore", "queue"]),
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
    Field {
        label: "voices",
        path: &["voices"],
        input: Input::Count,
        kinds: MEDIA,
        not: &[],
        needed: false,
    },
];

/// The layer keys the inspector leaves to other parts of the editor,
/// and which: the format's every key is either a property, a field
/// above, or here.
pub const ELSEWHERE: &[(&str, &str)] = &[
    ("type", "fixed once the layer is made"),
    ("name", "renames (item 28)"),
    ("id", "renames (item 28)"),
    ("children", "the layers list (item 24)"),
    ("parts", "the layers list (item 24)"),
    ("shape", "stage manipulation (item 23)"),
    ("clip", "stage manipulation (item 23)"),
    ("timelines", "the timelines (M3)"),
    ("when", "the timelines (M3)"),
    ("while", "the timelines (M3)"),
    ("bindings", "the binding card (M4)"),
    ("display", "its own editor, not planned yet"),
    ("sheet", "its own editor, not planned yet"),
    ("pick", "its own editor, not planned yet"),
    ("duck", "its own editor, not planned yet"),
    ("bus", "its own editor, not planned yet"),
];

/// The kind a layer's JSON is: its `type`, or `part` for an artwork
/// part, which has an `id` instead.
pub fn kind(layer: &Value) -> Option<&str> {
    layer
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| layer.get("id").map(|_| "part"))
}

/// The fields a layer of `kind` has.
pub fn of(kind: &str) -> impl Iterator<Item = &'static Field> + '_ {
    FIELDS.iter().filter(move |field| {
        (field.kinds.is_empty() || field.kinds.contains(&kind)) && !field.not.contains(&kind)
    })
}

/// What the layer writes for `field`, if anything.
pub fn read<'a>(layer: &'a Value, field: &Field) -> Option<&'a Value> {
    field
        .path
        .iter()
        .try_fold(layer, |node, key| node.get(key))
        .filter(|value| !value.is_null())
}

/// A field's value as its row shows it: a pair as `40, 30`, a colour
/// or a word as itself. A value the row cannot edit (a gradient fill,
/// a list of triggers) is `None`, and the row shows it read-only.
pub fn show(field: &Field, value: &Value) -> Option<String> {
    match (field.input, value) {
        (Input::Pair, Value::Array(pair)) => match pair.as_slice() {
            [a, b] => Some(format!("{a}, {b}")),
            _ => None,
        },
        (Input::Toggle, Value::Bool(b)) => Some(b.to_string()),
        (Input::Number | Input::Count, Value::Number(n)) => Some(n.to_string()),
        (_, Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// What was typed for `field`, as the value the document gets.
pub fn parse(field: &Field, typed: &str) -> Result<Value, String> {
    let typed = typed.trim();
    let number = |text: &str| -> Result<Value, String> {
        let n: f64 = text
            .trim()
            .parse()
            .map_err(|_| format!("{text:?} is not a number"))?;
        if !n.is_finite() {
            return Err(format!("{text:?} is not a number"));
        }
        Ok(if n.fract() == 0.0 && n.abs() < 1e15 {
            Value::from(n as i64)
        } else {
            Value::from(n)
        })
    };
    match field.input {
        Input::Choice(words) => words
            .contains(&typed)
            .then(|| Value::String(typed.to_owned()))
            .ok_or_else(|| format!("{typed:?} is not one of {}", words.join(", "))),
        Input::Toggle => match typed {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(format!("{typed:?} is not true or false")),
        },
        Input::Number => number(typed),
        Input::Count => typed
            .parse::<u64>()
            .map(Value::from)
            .map_err(|_| format!("{typed:?} is not a whole number")),
        Input::Pair => {
            let parts: Vec<&str> = typed.split(',').collect();
            match parts.as_slice() {
                [a, b] => Ok(Value::Array(vec![number(a)?, number(b)?])),
                _ => Err(format!("{typed:?} is not two numbers, like 40, 30")),
            }
        }
        Input::Colour => {
            crate::edit::parse(cuelight_core::Property::Tint, typed).and_then(|colour| match colour
            {
                Value::String(s) if !s.is_empty() => Ok(Value::String(s)),
                _ => Err("a colour is needed: #RRGGBB or #RRGGBBAA".to_owned()),
            })
        }
        Input::Text | Input::Artwork => {
            if typed.is_empty() {
                Err("a name is needed".to_owned())
            } else {
                Ok(Value::String(typed.to_owned()))
            }
        }
    }
}

/// Set `field` of the layer at `layer` to `value`: replace it where the
/// layer writes it, or add it, with the object it sits in (a stroke's
/// width with a stroke) when that is missing too.
pub fn set(
    document: &mut Document,
    layer: &Pointer,
    field: &Field,
    value: Value,
) -> Result<(), EditError> {
    let at = field.path.iter().fold(layer.clone(), |at, key| {
        at.then(Part::Key((*key).to_owned()))
    });
    if document.get(&at).is_some() {
        return document.set(&at, value);
    }
    // Down the path, the first key the layer does not write gets the
    // rest of the path built round the value.
    let mut parent = layer.clone();
    for (depth, key) in field.path.iter().enumerate() {
        let next = parent.then(Part::Key((*key).to_owned()));
        if document.get(&next).is_none() {
            let rest = field.path.get(depth + 1..).unwrap_or_default();
            let mut built = rest.iter().rev().fold(value, |inner, key| {
                let mut object = serde_json::Map::new();
                object.insert((*key).to_owned(), inner);
                Value::Object(object)
            });
            // A stroke is nothing without its colour: a width alone
            // starts it white.
            if *key == "stroke"
                && let Value::Object(stroke) = &mut built
            {
                stroke
                    .entry("color")
                    .or_insert_with(|| Value::from("#FFFFFF"));
            }
            return document.insert(&next, built);
        }
        parent = next;
    }
    Err(EditError::NotFound(at))
}

/// Whether `value` is what the layer `layer` (its JSON) has for
/// `field` when it does not write it: the engine is asked, with the key
/// taken out. A key the layer cannot do without has no default.
pub fn is_default(layer: &Value, field: &Field, value: &Value) -> bool {
    let mut without = layer.clone();
    let Some((last, parents)) = field.path.split_last() else {
        return false;
    };
    let parent = parents
        .iter()
        .try_fold(&mut without, |node, key| node.get_mut(*key));
    if let Some(object) = parent.and_then(Value::as_object_mut) {
        object.remove(*last);
    }
    let Ok(loaded) = serde_json::from_value::<cuelight_core::Layer>(without) else {
        return false;
    };
    let Ok(loaded) = serde_json::to_value(loaded) else {
        return false;
    };
    read(&loaded, field).is_some_and(|default| crate::edit::same(default, value))
}

/// Whether the layer at `layer` writes `field`, rather than leaving it to
/// its default.
pub fn written(document: &Document, layer: &Pointer, field: &Field) -> bool {
    document.get(&path_of(layer, field)).is_some()
}

/// Take `field` out of the layer at `layer`, back to its default, and
/// the object it sits in with it when that object needs it.
pub fn unset(document: &mut Document, layer: &Pointer, field: &Field) -> Result<(), EditError> {
    let keys = match field.path.split_last() {
        Some((_, parent)) if field.needed && !parent.is_empty() => parent,
        _ => field.path,
    };
    let at = keys.iter().fold(layer.clone(), |at, key| {
        at.then(Part::Key((*key).to_owned()))
    });
    document.remove(&at)
}

/// Where the layer at `layer` writes `field`.
fn path_of(layer: &Pointer, field: &Field) -> Pointer {
    field.path.iter().fold(layer.clone(), |at, key| {
        at.then(Part::Key((*key).to_owned()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(label: &str) -> &'static Field {
        FIELDS.iter().find(|f| f.label == label).unwrap()
    }

    #[test]
    fn typed_values_are_read_by_their_field() {
        assert_eq!(parse(field("size"), "40, 30"), Ok(json!([40, 30])));
        assert_eq!(parse(field("size"), "40.5,30"), Ok(json!([40.5, 30])));
        assert!(parse(field("size"), "40").is_err());
        assert_eq!(parse(field("blend"), "add"), Ok(json!("add")));
        assert!(parse(field("blend"), "darken").is_err());
        assert_eq!(parse(field("voices"), "3"), Ok(json!(3)));
        assert!(parse(field("voices"), "2.5").is_err());
        assert_eq!(parse(field("fill"), "#00ff00"), Ok(json!("#00FF00")));
        assert!(parse(field("fill"), "").is_err());
    }

    #[test]
    fn a_missing_object_is_built_round_the_value() {
        let mut document =
            Document::parse(r##"{"layers": [{"name": "r", "type": "shape", "fill": "#FFFFFF"}]}"##)
                .unwrap();
        let layer = Pointer::parse("/layers/0").unwrap();
        set(&mut document, &layer, field("stroke width"), json!(2)).unwrap();
        assert_eq!(
            document.value()["layers"][0]["stroke"],
            json!({"width": 2, "color": "#FFFFFF"}),
            "a stroke needs its colour"
        );
        set(&mut document, &layer, field("stroke"), json!("#FF0000")).unwrap();
        assert_eq!(
            document.value()["layers"][0]["stroke"],
            json!({"width": 2, "color": "#FF0000"})
        );
        assert!(document.undo() && document.undo());
        assert!(document.value()["layers"][0].get("stroke").is_none());
    }

    #[test]
    fn a_field_unset_takes_what_needs_it_along() {
        let mut document = Document::parse(
            r##"{"layers": [{"name": "r", "type": "shape", "fill": "#FFFFFF", "stroke": {"color": "#000000", "width": 2}}]}"##,
        )
        .unwrap();
        let layer = Pointer::parse("/layers/0").unwrap();
        unset(&mut document, &layer, field("stroke width")).unwrap();
        assert_eq!(
            document.value()["layers"][0]["stroke"],
            json!({"color": "#000000"})
        );
        unset(&mut document, &layer, field("stroke")).unwrap();
        assert!(
            document.value()["layers"][0].get("stroke").is_none(),
            "no stroke without a colour"
        );
        assert!(!written(&document, &layer, field("stroke")));
    }

    #[test]
    fn a_default_field_is_what_the_engine_reads_without_it() {
        let audio =
            json!({"name": "a", "type": "audio", "sound": "beep", "loop": true, "voices": 2});
        assert!(is_default(&audio, field("loop"), &json!(false)));
        assert!(!is_default(&audio, field("loop"), &json!(true)));
        assert!(is_default(&audio, field("blend"), &json!("normal")));
        let digits = json!({"name": "d", "type": "digits", "digits": 3, "text": "1",
                            "display": {"segments": {"style": "numeric7", "fill": "#FFFFFF"}}});
        assert!(
            !is_default(&digits, field("digits"), &json!(3)),
            "a digits count is required"
        );
    }

    #[test]
    fn a_gradient_or_a_list_is_shown_read_only() {
        let gradient = json!({"linear": {"from": [0, 0], "to": [1, 1], "stops": []}});
        assert_eq!(show(field("fill"), &gradient), None);
        assert_eq!(show(field("trigger"), &json!(["a", "b"])), None);
        assert_eq!(show(field("trigger"), &json!("go")), Some("go".to_owned()));
        assert_eq!(
            show(field("size"), &json!([40, 30])),
            Some("40, 30".to_owned())
        );
    }
}
