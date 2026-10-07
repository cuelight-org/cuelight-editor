//! A layer's fields that are not animatable properties: where it is
//! anchored, how it blends, a text's alignment, a shape's fill, a
//! sound's playback. The inspector edits them under the properties;
//! what they are, which kinds of layer have them and how each is typed
//! is said once, here. The show's own settings (its name, canvas and
//! output) are fields the same way, of the kind `show`.

use serde_json::Value;

use crate::document::{Document, EditError, Part, Pointer};

/// How a field is typed in the inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// One of these words.
    Choice(&'static [&'static str]),
    Toggle,
    Number,
    /// A number from 0 to 1: a share of something.
    Share,
    /// A whole number.
    Count,
    /// Two numbers, typed `40, 30`.
    Pair,
    /// `#RRGGBB` or `#RRGGBBAA`.
    Colour,
    /// `#RRGGBB`: a colour the format takes without alpha.
    Opaque,
    Text,
    /// The name of one of the show's images or vector artwork.
    Artwork,
    /// The name of one of the show's font files.
    Font,
}

/// A field of a layer: where the layer writes it, how it is typed, and
/// which kinds of layer have it (empty for every kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// Its name in the inspector.
    pub label: &'static str,
    /// The keys down to it from the layer's object: `["stroke", "width"]`;
    /// a number steps into a list (`["passes", "0", "dots"]`).
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
        label: "box",
        path: &["box"],
        input: Input::Choice(&["line", "cap"]),
        kinds: &["text"],
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
        label: "fit",
        path: &["fit"],
        input: Input::Choice(&["fill", "contain", "cover"]),
        kinds: &["image", "vector"],
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
/// The show's own settings, in the order the inspector lists them.
pub const SHOW_FIELDS: &[Field] = &[
    show_field("name", &["name"], Input::Text),
    show_field("size", &["size"], Input::Pair),
    show_field("background", &["background"], Input::Colour),
    show_field(
        "mode",
        &["output", "mode"],
        Input::Choice(&["rgb", "gray2", "gray4"]),
    ),
    show_field("tint", &["output", "tint"], Input::Opaque),
    show_field(
        "scaling",
        &["output", "scaling"],
        Input::Choice(&["smooth", "pixel_perfect"]),
    ),
    show_field(
        "edges",
        &["output", "edges"],
        Input::Choice(&["soft", "hard"]),
    ),
    // The dot matrix pass: setting any of these shows the frame as dots,
    // taking them all out shows it plain.
    show_field(
        "dot size",
        &["output", "passes", "0", "dots", "size"],
        Input::Share,
    ),
    show_field(
        "dot shape",
        &["output", "passes", "0", "dots", "shape"],
        Input::Choice(&["round", "square"]),
    ),
    show_field(
        "unlit",
        &["output", "passes", "0", "dots", "unlit"],
        Input::Opaque,
    ),
    show_field(
        "glow",
        &["output", "passes", "0", "dots", "glow"],
        Input::Share,
    ),
    show_field("press", &["input", "press"], Input::Text),
    show_field("pointer x", &["input", "pointer", "x"], Input::Text),
    show_field("pointer y", &["input", "pointer", "y"], Input::Text),
    show_field("pointer over", &["input", "pointer", "over"], Input::Text),
    show_field("pointer under", &["input", "pointer", "under"], Input::Text),
];

const fn show_field(label: &'static str, path: &'static [&'static str], input: Input) -> Field {
    Field {
        label,
        path,
        input,
        kinds: &["show"],
        not: &[],
        needed: false,
    }
}

/// A font style's keys, in the order the inspector lists them.
pub const STYLE_FIELDS: &[Field] = &[
    style_field("file", &["file"], Input::Font, false),
    style_field("size", &["size"], Input::Number, false),
    style_field("color", &["color"], Input::Opaque, false),
    style_field("border", &["border", "color"], Input::Opaque, true),
    style_field("border width", &["border", "width"], Input::Count, false),
    style_field("shadow", &["shadow", "color"], Input::Colour, true),
    style_field("shadow offset", &["shadow", "offset"], Input::Pair, true),
    style_field("shadow blur", &["shadow", "blur"], Input::Number, false),
    style_field("pixels", &["pixels"], Input::Toggle, false),
];

const fn style_field(
    label: &'static str,
    path: &'static [&'static str],
    input: Input,
    needed: bool,
) -> Field {
    Field {
        label,
        path,
        input,
        kinds: &["font style"],
        not: &[],
        needed,
    }
}

/// The show's keys the inspector does not edit as settings, and what
/// edits them instead.
pub const SHOW_ELSEWHERE: &[(&str, &str)] = &[
    ("format", "fixed by the engine the show is written for"),
    ("fonts", "the font styles"),
    ("values", "the binding card (M4)"),
    ("layers", "the layers list (item 24)"),
    ("scenes", "the layers list (item 24)"),
    ("variables", "the show's lists"),
    ("keys", "the show's lists"),
];

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

/// The fields a layer of `kind` has, or the show's settings for `show`.
pub fn of(kind: &str) -> impl Iterator<Item = &'static Field> + '_ {
    let table = match kind {
        "show" => SHOW_FIELDS,
        "font style" => STYLE_FIELDS,
        _ => FIELDS,
    };
    table.iter().filter(move |field| {
        (field.kinds.is_empty() || field.kinds.contains(&kind)) && !field.not.contains(&kind)
    })
}

/// What the layer writes for `field`, if anything.
pub fn read<'a>(layer: &'a Value, field: &Field) -> Option<&'a Value> {
    field
        .path
        .iter()
        .try_fold(layer, |node, key| match key.parse::<usize>() {
            Ok(i) => node.get(i),
            Err(_) => node.get(key),
        })
        .filter(|value| !value.is_null())
}

/// One key of a field's path as a pointer's step: a number steps into a
/// list.
fn step(key: &str) -> Part {
    key.parse()
        .map(Part::Index)
        .unwrap_or_else(|_| Part::Key(key.to_owned()))
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
        (Input::Number | Input::Share | Input::Count, Value::Number(n)) => Some(n.to_string()),
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
        Input::Share => number(typed).and_then(|n| match n.as_f64() {
            Some(share) if (0.0..=1.0).contains(&share) => Ok(n),
            _ => Err(format!("{typed:?} is not from 0 to 1")),
        }),
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
        Input::Opaque => {
            crate::edit::parse(cuelight_core::Property::Tint, typed).and_then(|colour| match colour
            {
                Value::String(s) if s.len() == 7 => Ok(Value::String(s)),
                _ => Err("a colour without alpha is needed: #RRGGBB".to_owned()),
            })
        }
        Input::Text | Input::Artwork | Input::Font => {
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
    let at = path_of(layer, field);
    if document.get(&at).is_some() {
        return document.set(&at, value);
    }
    // Down the path, the first key the layer does not write gets the
    // rest of the path built round the value.
    let mut parent = layer.clone();
    for (depth, key) in field.path.iter().enumerate() {
        let next = parent.then(step(key));
        if document.get(&next).is_none() {
            let rest = field.path.get(depth + 1..).unwrap_or_default();
            let mut built = rest.iter().rev().fold(value, |inner, key| {
                if key.parse::<usize>().is_ok() {
                    return Value::Array(vec![inner]);
                }
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
            // A font style's border needs its colour, and its shadow its
            // colour and offset: one set alone starts them dark, two
            // pixels down and right.
            if field.kinds == ["font style"]
                && let Value::Object(object) = &mut built
            {
                match *key {
                    "border" => {
                        object
                            .entry("color")
                            .or_insert_with(|| Value::from("#000000"));
                    }
                    "shadow" => {
                        object
                            .entry("color")
                            .or_insert_with(|| Value::from("#000000"));
                        object
                            .entry("offset")
                            .or_insert_with(|| serde_json::json!([2, 2]));
                    }
                    _ => {}
                }
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
    if field.kinds == ["show"] {
        return show_default(field).is_some_and(|default| crate::edit::same(&default, value));
    }
    if field.kinds == ["font style"] {
        return style_default(layer, field)
            .is_some_and(|default| crate::edit::same(&default, value));
    }
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

/// What a show has for `field` when it does not write it: the format's
/// default, or what a host takes for an output setting left out. A show
/// cannot do without its name and size, which have none.
pub fn show_default(field: &Field) -> Option<Value> {
    use cuelight_core::{Edges, OutputMode, Scaling, Show};
    if matches!(field.path, ["name"] | ["size"]) {
        return None;
    }
    let bare: Show =
        serde_json::from_value(serde_json::json!({"name": "", "size": [1, 1]})).ok()?;
    let mut bare = serde_json::to_value(bare).ok()?;
    *bare.get_mut("output")? = serde_json::json!({
        "mode": OutputMode::default(),
        "tint": "#FFFFFF",
        "scaling": Scaling::default(),
        "edges": Edges::default(),
    });
    read(&bare, field).cloned()
}

/// What the font style `style` (its JSON) has for `field` when it does
/// not write it, as the engine reads it without the key. Its file has
/// none.
pub fn style_default(style: &Value, field: &Field) -> Option<Value> {
    let (last, parents) = field.path.split_last()?;
    let mut without = style.clone();
    let parent = parents
        .iter()
        .try_fold(&mut without, |node, key| node.get_mut(*key));
    if let Some(object) = parent.and_then(Value::as_object_mut) {
        object.remove(*last);
    }
    let loaded: cuelight_core::FontStyle = serde_json::from_value(without).ok()?;
    let loaded = serde_json::to_value(loaded).ok()?;
    read(&loaded, field).cloned()
}

/// What the document writes for `field` of the layer (or show) at
/// `layer`, if anything.
pub fn written_value(document: &Document, layer: &Pointer, field: &Field) -> Option<Value> {
    document
        .get(&path_of(layer, field))
        .map(|node| node.value())
        .filter(|value| !value.is_null())
}

/// Whether the layer at `layer` writes `field`, rather than leaving it to
/// its default.
pub fn written(document: &Document, layer: &Pointer, field: &Field) -> bool {
    document.get(&path_of(layer, field)).is_some()
}

/// Take `field` out of the layer at `layer`, back to its default, and
/// the object it sits in with it when that object needs it. What that
/// leaves empty (an `input` with nothing in it) goes too.
pub fn unset(document: &mut Document, layer: &Pointer, field: &Field) -> Result<(), EditError> {
    let keys = match field.path.split_last() {
        Some((_, parent)) if field.needed && !parent.is_empty() => parent,
        _ => field.path,
    };
    remove_path(document, layer, keys)
}

/// Take out what `keys` reach from `owner`, and then each object or list
/// above it that it leaves empty, up to `owner` itself.
pub fn remove_path(
    document: &mut Document,
    owner: &Pointer,
    keys: &[&str],
) -> Result<(), EditError> {
    let at = keys
        .iter()
        .fold(owner.clone(), |at, key| at.then(step(key)));
    document.remove(&at)?;
    for depth in (1..keys.len()).rev() {
        let parent = keys
            .iter()
            .take(depth)
            .fold(owner.clone(), |at, key| at.then(step(key)));
        let empty = document
            .get(&parent)
            .is_some_and(|node| match node.value() {
                Value::Object(object) => object.is_empty(),
                Value::Array(items) => items.is_empty(),
                _ => false,
            });
        if !empty {
            break;
        }
        document.remove(&parent)?;
    }
    Ok(())
}

/// Where the layer at `layer` writes `field`.
fn path_of(layer: &Pointer, field: &Field) -> Pointer {
    field
        .path
        .iter()
        .fold(layer.clone(), |at, key| at.then(step(key)))
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
        let glow = SHOW_FIELDS.iter().find(|f| f.label == "glow").unwrap();
        assert_eq!(parse(glow, "0.3"), Ok(json!(0.3)));
        assert!(parse(glow, "1.5").is_err(), "a share is at most 1");
        assert!(parse(glow, "-0.1").is_err());
        let unlit = SHOW_FIELDS.iter().find(|f| f.label == "unlit").unwrap();
        assert_eq!(parse(unlit, "#1a0904"), Ok(json!("#1A0904")));
        assert!(parse(unlit, "#1A090480").is_err(), "unlit has no alpha");
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
