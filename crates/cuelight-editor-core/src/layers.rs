//! Making and arranging layers in the document: a new layer of each
//! kind, and deleting, duplicating, reordering, grouping and moving
//! them, each as one undo step. Layers are addressed by their pointers
//! in the document; what an edit made is given back the same way.
//!
//! Triggers and bindings address a layer by its name, so a new or
//! copied layer gets one its siblings do not have: `rect`, then
//! `rect_2`, `rect_3`; a copy of `title` is `title_2`. A move that would
//! put two layers of one name side by side is refused instead, since
//! renaming one would change what addresses it.

use std::cmp::Ordering;

use serde_json::{Value, json};

use crate::document::{Document, EditError, Part, Pointer};

/// A kind of layer the insert menu makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Rectangle,
    RoundedRectangle,
    Circle,
    /// A shape from SVG path data.
    Path,
    Text,
    Digits,
    /// A picture or SVG artwork from the show's assets.
    Image,
    Group,
    Audio,
    Video,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Rectangle,
        Kind::RoundedRectangle,
        Kind::Circle,
        Kind::Path,
        Kind::Text,
        Kind::Digits,
        Kind::Image,
        Kind::Group,
        Kind::Audio,
        Kind::Video,
    ];

    /// What the menu calls it.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Rectangle => "Rectangle",
            Kind::RoundedRectangle => "Rounded rectangle",
            Kind::Circle => "Circle",
            Kind::Path => "Path",
            Kind::Text => "Text",
            Kind::Digits => "Digits",
            Kind::Image => "Image",
            Kind::Group => "Group",
            Kind::Audio => "Audio",
            Kind::Video => "Video",
        }
    }
}

/// What a new layer is made from: the canvas it is centred on, and
/// what the show has for the kinds that need something of it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Making {
    /// The canvas size.
    pub size: [f64; 2],
    /// The show's first font style, for text.
    pub font: Option<String>,
    /// The first sound, for an audio layer.
    pub sound: Option<String>,
    /// The first video, for a video layer.
    pub video: Option<String>,
    /// The first picture or SVG artwork, for an image layer: its name,
    /// whether it is SVG, and its size when known.
    pub artwork: Option<(String, bool, Option<[f64; 2]>)>,
    /// SVG path data, for a path.
    pub path: String,
}

/// A new layer's colour: a shape's fill.
const FILL: &str = "#4F8FD8";

/// A unit for a new layer's size: an eighth of the canvas's shorter
/// side, in whole pixels, so what is made fits whatever the canvas.
fn unit(size: [f64; 2]) -> f64 {
    (size[0].min(size[1]) / 8.0).round().max(1.0)
}

/// The middle of the canvas, in whole pixels.
fn middle(size: [f64; 2]) -> [f64; 2] {
    [(size[0] / 2.0).round(), (size[1] / 2.0).round()]
}

/// A number as JSON writes it: `40` rather than `40.0` when whole.
fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        Value::from(n as i64)
    } else {
        Value::from(n)
    }
}

/// A triangle round the layer's origin, as SVG path data: what the path
/// field starts with.
pub fn default_path(size: [f64; 2]) -> String {
    let u = unit(size) * 2.0;
    format!("M 0 -{u} L {u} {u} L -{u} {u} Z")
}

/// Why a layer of `kind` cannot be made, or `None` when it can.
pub fn unavailable(kind: Kind, making: &Making) -> Option<&'static str> {
    match kind {
        // A text layer without a style the show declares is left out
        // at load, so the tree would never show it; the editor makes a
        // style for a show that has a font to make one with.
        Kind::Text if making.font.is_none() => Some("the show has no fonts to write in"),
        Kind::Audio if making.sound.is_none() => Some("the show has no sounds to play"),
        Kind::Video if making.video.is_none() => Some("the show has no videos to show"),
        Kind::Image if making.artwork.is_none() => Some("the show has no pictures or SVG artwork"),
        Kind::Path if making.path.trim().is_empty() => Some("a path needs its path data"),
        _ => None,
    }
}

/// A new layer of `kind` with the keys it needs and no more: its name,
/// its type, where it sits (the middle of the canvas, for what draws),
/// then what the kind needs. Its name is told apart from its siblings'
/// when it is inserted.
pub fn new_layer(kind: Kind, making: &Making) -> Result<Value, String> {
    if let Some(why) = unavailable(kind, making) {
        return Err(why.to_owned());
    }
    let u = unit(making.size);
    let [x, y] = middle(making.size).map(number);
    let shape = |name: &str, shape: Value| json!({ "name": name, "type": "shape", "x": x, "y": y, "shape": shape, "fill": FILL });
    Ok(match kind {
        Kind::Rectangle => shape(
            "rect",
            json!({ "rect": [number(-2.0 * u), number(-u), number(4.0 * u), number(2.0 * u)] }),
        ),
        Kind::RoundedRectangle => shape(
            "rounded_rect",
            json!({ "rect": [number(-2.0 * u), number(-u), number(4.0 * u), number(2.0 * u)],
                    "radius": number((u / 2.0).round().max(1.0)) }),
        ),
        Kind::Circle => shape("circle", json!({ "circle": [0, 0, number(2.0 * u)] })),
        Kind::Path => shape("path", json!({ "path": making.path.trim() })),
        Kind::Text => json!({
            "name": "text", "type": "text", "x": x, "y": y, "anchor": "center",
            "text": "Text", "font": making.font.clone().unwrap_or_default()
        }),
        Kind::Digits => {
            let cell = (1.5 * u).round();
            json!({
                "name": "digits", "type": "digits", "x": x, "y": y, "anchor": "center",
                "digits": 4, "size": [number(4.0 * cell), number(2.0 * cell)], "text": "1234",
                "display": { "segments": { "style": "numeric7", "fill": "#FF5820", "unlit": "#2A0E05" } }
            })
        }
        // At its own size, or half the canvas when it is larger, its
        // shape kept; an SVG layer names its artwork as `vector`.
        Kind::Image => {
            let (name, vector, size) = making.artwork.clone().unwrap_or_default();
            let (key, kind) = if vector {
                ("vector", "vector")
            } else {
                ("image", "image")
            };
            let mut layer =
                json!({ "name": name, "type": kind, "x": x, "y": y, "anchor": "center" });
            if let (Some([w, h]), Some(object)) = (size, layer.as_object_mut()) {
                let room = [making.size[0] / 2.0, making.size[1] / 2.0];
                let shrink = (room[0] / w.max(1.0)).min(room[1] / h.max(1.0));
                if shrink < 1.0 {
                    object.insert(
                        "size".to_owned(),
                        json!([number((w * shrink).round()), number((h * shrink).round())]),
                    );
                }
            }
            if let Some(object) = layer.as_object_mut() {
                object.insert(key.to_owned(), Value::from(name));
            }
            layer
        }
        Kind::Group => json!({ "name": "group", "type": "group", "children": [] }),
        Kind::Audio => {
            let sound = making.sound.as_deref().unwrap_or_default();
            json!({ "name": sound, "type": "audio", "sound": sound })
        }
        Kind::Video => {
            let video = making.video.as_deref().unwrap_or_default();
            json!({ "name": video, "type": "video", "video": video })
        }
    })
}

/// `wanted` if no name in `taken` is, or else the first of `base_2`,
/// `base_3`, ... that is free, `base` being `wanted` without a number
/// it ends with: a second `title_2` is `title_3`.
pub fn unique_name(taken: &[String], wanted: &str) -> String {
    if !taken.iter().any(|t| t == wanted) {
        return wanted.to_owned();
    }
    let base = match wanted.rsplit_once('_') {
        Some((base, n)) if !base.is_empty() && n.parse::<u32>().is_ok() => base,
        _ => wanted,
    };
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_else(|| wanted.to_owned())
}

/// The list a layer's pointer leads into, and its index there.
pub fn place_of(layer: &Pointer) -> Option<(Pointer, usize)> {
    match layer.0.split_last()? {
        (Part::Index(i), list) => Some((Pointer(list.to_vec()), *i)),
        _ => None,
    }
}

/// The list of the layers inside a group at `group`.
pub fn children_of(group: &Pointer) -> Pointer {
    group.then(Part::Key("children".to_owned()))
}

/// Whether the pointer leads to a part of an artwork: those belong to
/// their artwork and are only taken out, never copied or moved.
fn is_part(layer: &Pointer) -> bool {
    matches!(layer.0.iter().rev().nth(1), Some(Part::Key(list)) if list == "parts")
}

/// The names of the layers in `list`, in order (a part's id stands in
/// for its name).
fn names(document: &Document, list: &Pointer) -> Vec<String> {
    let Some(Value::Array(layers)) = document.get(list).map(|n| n.value()) else {
        return Vec::new();
    };
    layers.iter().map(name_of).collect()
}

fn name_of(layer: &Value) -> String {
    layer
        .get("name")
        .or_else(|| layer.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// How many items the array at `list` has.
fn length(document: &Document, list: &Pointer) -> usize {
    match document.get(list).map(|n| n.value()) {
        Some(Value::Array(items)) => items.len(),
        _ => 0,
    }
}

/// Pointers in document order, the way the tree lists them.
fn order(a: &Pointer, b: &Pointer) -> Ordering {
    for (x, y) in a.0.iter().zip(&b.0) {
        let by = match (x, y) {
            (Part::Index(x), Part::Index(y)) => x.cmp(y),
            (Part::Key(x), Part::Key(y)) => x.cmp(y),
            (Part::Key(_), Part::Index(_)) => Ordering::Less,
            (Part::Index(_), Part::Key(_)) => Ordering::Greater,
        };
        if by != Ordering::Equal {
            return by;
        }
    }
    a.0.len().cmp(&b.0.len())
}

/// The picked layers in document order, once each, leaving out any
/// inside another one picked: what is done to a group is done to all
/// of it.
fn outermost(layers: &[Pointer]) -> Vec<Pointer> {
    let mut sorted = layers.to_vec();
    sorted.sort_by(order);
    sorted.dedup();
    let mut out: Vec<Pointer> = Vec::new();
    for layer in sorted {
        if !out.iter().any(|o| layer.0.starts_with(&o.0)) {
            out.push(layer);
        }
    }
    out
}

/// `pointer` after an item went in at `at` (`by` 1) or came out of it
/// (`by` -1): a later item of the same list, or anything inside one,
/// moves along.
fn shift(pointer: &mut Pointer, at: &Pointer, by: isize) {
    let Some((list, i)) = place_of(at) else {
        return;
    };
    if pointer.0.len() <= list.0.len() || !pointer.0.starts_with(&list.0) {
        return;
    }
    if let Some(Part::Index(k)) = pointer.0.get_mut(list.0.len())
        && (*k > i || (by > 0 && *k == i))
    {
        *k = k.saturating_add_signed(by);
    }
}

/// Run `edit` as one undo step. An edit that fails half way is taken
/// back, so the document is as it was.
fn step<T>(
    document: &mut Document,
    edit: impl FnOnce(&mut Document) -> Result<T, String>,
) -> Result<T, String> {
    let revision = document.revision();
    document.begin_step();
    let done = edit(document);
    document.end_step();
    if done.is_err() && document.revision() != revision {
        document.undo();
    }
    done
}

fn said(error: EditError) -> String {
    error.to_string()
}

/// Make the list at `list` when the show or a scene does not have one
/// yet: `"layers": []`. Anything else without a list is no group.
fn ensure_list(document: &mut Document, list: &Pointer) -> Result<(), String> {
    if document.get(list).is_some() {
        return Ok(());
    }
    let root = matches!(list.0.as_slice(), [Part::Key(layers)] if layers == "layers")
        || matches!(list.0.as_slice(),
            [Part::Key(scenes), Part::Index(_), Part::Key(layers)]
                if scenes == "scenes" && layers == "layers");
    if !root {
        return Err("only a group holds layers".to_owned());
    }
    document.insert(list, json!([])).map_err(said)
}

/// Put `layer` into `list` at `index` (its length appends), named apart
/// from what is there. The list is made when the show or scene has
/// none. Gives where it went.
pub fn insert(
    document: &mut Document,
    list: &Pointer,
    index: usize,
    layer: Value,
) -> Result<Pointer, String> {
    step(document, |document| {
        ensure_list(document, list)?;
        let mut layer = layer;
        if let Some(fields) = layer.as_object_mut() {
            let wanted = fields
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("layer")
                .to_owned();
            let name = unique_name(&names(document, list), &wanted);
            fields.insert("name".to_owned(), Value::from(name));
        }
        let index = index.min(length(document, list));
        let at = list.then(Part::Index(index));
        document.insert_item(&at, layer).map_err(said)?;
        Ok(at)
    })
}

/// Take the layers out.
pub fn delete(document: &mut Document, layers: &[Pointer]) -> Result<(), String> {
    let layers = outermost(layers);
    step(document, |document| {
        // Last first, so the ones before keep their places.
        for layer in layers.iter().rev() {
            document.remove(layer).map_err(said)?;
        }
        Ok(())
    })
}

/// Put a copy of each layer right after it, named apart from its
/// siblings. Gives where the copies went, in document order.
pub fn duplicate(document: &mut Document, layers: &[Pointer]) -> Result<Vec<Pointer>, String> {
    let layers = outermost(layers);
    if layers.iter().any(is_part) {
        return Err("a part belongs to its artwork and is not copied".to_owned());
    }
    step(document, |document| {
        let mut copies: Vec<Pointer> = Vec::new();
        for layer in layers.iter().rev() {
            let (list, i) = place_of(layer).ok_or("not a layer")?;
            let wanted = document
                .get(layer)
                .map(|n| name_of(&n.value()))
                .unwrap_or_default();
            let name = unique_name(&names(document, &list), &wanted);
            let copy = list.then(Part::Index(i + 1));
            document.copy_item(layer, &copy).map_err(said)?;
            let named = copy.then(Part::Key("name".to_owned()));
            if document.get(&named).is_some() {
                document.set(&named, Value::from(name)).map_err(said)?;
            } else {
                document.insert(&named, Value::from(name)).map_err(said)?;
            }
            for earlier in &mut copies {
                shift(earlier, &copy, 1);
            }
            copies.push(copy);
        }
        copies.reverse();
        Ok(copies)
    })
}

/// The list the layers all share, or why they do not.
fn one_list(layers: &[Pointer]) -> Result<(Pointer, Vec<usize>), String> {
    let mut shared: Option<Pointer> = None;
    let mut indices = Vec::new();
    for layer in layers {
        let (list, i) = place_of(layer).ok_or("not a layer")?;
        match &shared {
            Some(s) if *s != list => return Err("pick layers side by side in one list".to_owned()),
            _ => shared = Some(list),
        }
        indices.push(i);
    }
    let list = shared.ok_or("nothing is picked")?;
    Ok((list, indices))
}

/// Move the layers, side by side in one list, one place earlier (`up`,
/// further back) or later (further forward). Gives where they are now.
pub fn reorder(
    document: &mut Document,
    layers: &[Pointer],
    up: bool,
) -> Result<Vec<Pointer>, String> {
    let layers = outermost(layers);
    let (list, mut indices) = one_list(&layers)?;
    indices.sort_unstable();
    let len = length(document, &list);
    if up && indices.first() == Some(&0) {
        return Err("already first".to_owned());
    }
    if !up && indices.last().is_some_and(|last| last + 1 >= len) {
        return Err("already last".to_owned());
    }
    step(document, |document| {
        let mut moved = Vec::new();
        // The one leading the way first, so each lands on a free place.
        let ordered: Vec<usize> = if up {
            indices.clone()
        } else {
            indices.iter().rev().copied().collect()
        };
        for i in ordered {
            let to = if up { i - 1 } else { i + 1 };
            document
                .move_item(&list.then(Part::Index(i)), &list.then(Part::Index(to)))
                .map_err(said)?;
            moved.push(list.then(Part::Index(to)));
        }
        moved.sort_by(order);
        Ok(moved)
    })
}

/// Put the layers, side by side in one list, into a new group where the
/// first of them was, keeping their order. Gives the group.
pub fn group(document: &mut Document, layers: &[Pointer]) -> Result<Pointer, String> {
    let layers = outermost(layers);
    if layers.iter().any(is_part) {
        return Err("a part belongs to its artwork and stays in it".to_owned());
    }
    let (list, indices) = one_list(&layers)?;
    let first = indices.iter().copied().min().ok_or("nothing is picked")?;
    step(document, |document| {
        let name = unique_name(&names(document, &list), "group");
        let group = list.then(Part::Index(first));
        document
            .insert_item(
                &group,
                json!({ "name": name, "type": "group", "children": [] }),
            )
            .map_err(said)?;
        // Each is one further on for the group, and one nearer for
        // each moved before it.
        for (k, i) in indices.iter().enumerate() {
            let from = list.then(Part::Index(i + 1 - k));
            let to = children_of(&group).then(Part::Index(k));
            document.move_item(&from, &to).map_err(said)?;
        }
        Ok(group)
    })
}

/// The keys a group may write and still be taken apart: it adds nothing
/// to its children that would be lost.
const BARE_GROUP: [&str; 3] = ["name", "type", "children"];

/// Put a group's children where it was, in their order, and take the
/// group out. Refused for a group that writes anything of its own (a
/// place, an opacity, a clip, a timeline...), since its children would
/// lose it. Gives where the children went.
pub fn ungroup(document: &mut Document, group: &Pointer) -> Result<Vec<Pointer>, String> {
    let value = document
        .get(group)
        .map(|n| n.value())
        .ok_or("not a layer")?;
    if value.get("type").and_then(Value::as_str) != Some("group") {
        return Err("not a group".to_owned());
    }
    let own: Vec<&str> = value
        .as_object()
        .map(|fields| {
            fields
                .keys()
                .map(String::as_str)
                .filter(|key| !BARE_GROUP.contains(key))
                .collect()
        })
        .unwrap_or_default();
    if !own.is_empty() {
        return Err(format!(
            "the group has its own {}, which its children would lose",
            own.join(", ")
        ));
    }
    let (list, i) = place_of(group).ok_or("not a layer")?;
    let children = children_of(group);
    let count = length(document, &children);
    let mut taken = names(document, &list);
    taken.remove(i);
    if let Some(clash) = names(document, &children)
        .iter()
        .find(|n| taken.contains(n))
    {
        return Err(format!("a layer named {clash} is beside the group already"));
    }
    step(document, |document| {
        for k in 0..count {
            document
                .move_item(
                    &children.then(Part::Index(0)),
                    &list.then(Part::Index(i + 1 + k)),
                )
                .map_err(said)?;
        }
        document.remove(group).map_err(said)?;
        Ok((0..count).map(|k| list.then(Part::Index(i + k))).collect())
    })
}

/// Move the layers to the end of `list`: into a group (its
/// `children`), out to the show's layers or into a scene's. Refused
/// for a part, for a group moved into itself, and where two layers of
/// one name would end up side by side. Gives where they went.
pub fn move_into(
    document: &mut Document,
    layers: &[Pointer],
    list: &Pointer,
) -> Result<Vec<Pointer>, String> {
    let layers = outermost(layers);
    if layers.iter().any(is_part) {
        return Err("a part belongs to its artwork and stays in it".to_owned());
    }
    if layers.iter().any(|l| list.0.starts_with(&l.0)) {
        return Err("a group cannot go inside itself".to_owned());
    }
    let mut seen: Vec<String> = names(document, list)
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !layers.contains(&list.then(Part::Index(*i))))
        .map(|(_, name)| name)
        .collect();
    for layer in &layers {
        let name = document
            .get(layer)
            .map(|n| name_of(&n.value()))
            .unwrap_or_default();
        if seen.contains(&name) {
            return Err(format!("two layers named {name} would be side by side"));
        }
        seen.push(name);
    }
    step(document, |document| {
        ensure_list(document, list)?;
        let mut list = list.clone();
        let mut left = layers.clone();
        let mut moved: Vec<Pointer> = Vec::new();
        while !left.is_empty() {
            let from = left.remove(0);
            let (from_list, _) = place_of(&from).ok_or("not a layer")?;
            let end = length(document, &list) - usize::from(from_list == list);
            // Where the list is once `from` is out: it may be after it.
            for p in left.iter_mut().chain(moved.iter_mut()) {
                shift(p, &from, -1);
            }
            shift(&mut list, &from, -1);
            let to = list.then(Part::Index(end));
            document.move_item(&from, &to).map_err(said)?;
            for p in &mut left {
                shift(p, &to, 1);
            }
            moved.push(to);
        }
        Ok(moved)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Pointer {
        Pointer::parse(text).unwrap()
    }

    fn names_at(document: &Document, list: &str) -> Vec<String> {
        names(document, &p(list))
    }

    fn making() -> Making {
        Making {
            size: [200.0, 100.0],
            font: Some("body".to_owned()),
            sound: Some("ding".to_owned()),
            video: None,
            artwork: Some(("badge".to_owned(), false, Some([40.0, 40.0]))),
            path: "M 0 0 L 10 0 L 5 8 Z".to_owned(),
        }
    }

    #[test]
    fn an_image_keeps_its_size_or_shrinks_to_half_the_canvas() {
        let small = new_layer(Kind::Image, &making()).unwrap();
        assert_eq!(
            small,
            json!({"name": "badge", "type": "image", "x": 100, "y": 50, "anchor": "center", "image": "badge"})
        );
        let big = Making {
            artwork: Some(("sky".to_owned(), true, Some([400.0, 100.0]))),
            ..making()
        };
        let big = new_layer(Kind::Image, &big).unwrap();
        assert_eq!(big["type"], "vector");
        assert_eq!(big["vector"], "sky");
        assert_eq!(
            big["size"],
            json!([100, 25]),
            "half the canvas wide, its shape kept"
        );
        let none = Making {
            artwork: None,
            ..making()
        };
        assert_eq!(
            new_layer(Kind::Image, &none).unwrap_err(),
            "the show has no pictures or SVG artwork"
        );
    }

    #[test]
    fn a_name_is_told_apart_from_its_siblings() {
        let taken: Vec<String> = ["rect", "title", "title_2", "x_1"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(unique_name(&taken, "circle"), "circle");
        assert_eq!(unique_name(&taken, "rect"), "rect_2");
        assert_eq!(unique_name(&taken, "title"), "title_3");
        assert_eq!(unique_name(&taken, "title_2"), "title_3");
        assert_eq!(unique_name(&taken, "x_1"), "x_2");
    }

    #[test]
    fn a_layer_goes_into_an_empty_show_on_lines_of_its_own() {
        let mut document =
            Document::parse("{\n  \"format\": 1,\n  \"name\": \"t\",\n  \"size\": [200, 100]\n}\n")
                .unwrap();
        let rect = new_layer(Kind::Rectangle, &making()).unwrap();
        let at = insert(&mut document, &p("/layers"), 0, rect.clone()).unwrap();
        assert_eq!(at, p("/layers/0"));
        let again = insert(&mut document, &p("/layers"), 9, rect).unwrap();
        assert_eq!(again, p("/layers/1"));
        assert_eq!(names_at(&document, "/layers"), ["rect", "rect_2"]);
        assert_eq!(
            document.text(),
            r##"{
  "format": 1,
  "name": "t",
  "size": [200, 100],
  "layers": [
    {
      "name": "rect",
      "type": "shape",
      "x": 100,
      "y": 50,
      "shape": {
        "rect": [-26, -13, 52, 26]
      },
      "fill": "#4F8FD8"
    },
    {
      "name": "rect_2",
      "type": "shape",
      "x": 100,
      "y": 50,
      "shape": {
        "rect": [-26, -13, 52, 26]
      },
      "fill": "#4F8FD8"
    }
  ]
}
"##
        );
        assert!(document.undo(), "each insert is one step");
        assert!(document.undo());
        assert!(document.value().get("layers").is_none());
        assert!(!document.undo());
    }

    #[test]
    fn every_kind_is_made_or_says_why_not() {
        for kind in Kind::ALL {
            match new_layer(kind, &making()) {
                Ok(layer) => {
                    let keys: Vec<&String> = layer.as_object().unwrap().keys().collect();
                    assert_eq!(keys[..2], ["name", "type"], "{kind:?}");
                }
                Err(why) => {
                    assert_eq!(kind, Kind::Video);
                    assert_eq!(why, "the show has no videos to show");
                }
            }
        }
        let text = new_layer(Kind::Text, &making()).unwrap();
        assert_eq!(text["font"], "body");
        let path = new_layer(Kind::Path, &making()).unwrap();
        assert_eq!(path["shape"]["path"], "M 0 0 L 10 0 L 5 8 Z");
        assert_eq!(default_path([200.0, 100.0]), "M 0 -26 L 26 26 L -26 26 Z");
    }

    fn show() -> Document {
        Document::parse(
            r#"{
  "name": "t",
  "layers": [
    { "name": "a", "type": "group", "children": [
      { "name": "a1", "type": "shape" },
      { "name": "a2", "type": "shape" }
    ] },
    { "name": "b", "type": "shape" },
    { "name": "c", "type": "shape" },
    { "name": "d", "type": "shape" }
  ],
  "scenes": [ { "name": "s", "layers": [ { "name": "b", "type": "shape" } ] } ]
}"#,
        )
        .unwrap()
    }

    #[test]
    fn layers_are_deleted_together_and_come_back_with_one_undo() {
        let mut document = show();
        let before = document.text();
        delete(
            &mut document,
            &[
                p("/layers/2"),
                p("/layers/0/children/1"),
                p("/layers/0"),
                p("/layers/3"),
            ],
        )
        .unwrap();
        assert_eq!(names_at(&document, "/layers"), ["b"]);
        assert!(document.undo());
        assert_eq!(document.text(), before);
    }

    #[test]
    fn a_copy_comes_right_after_its_layer_with_a_name_of_its_own() {
        let mut document = show();
        let copies = duplicate(&mut document, &[p("/layers/2"), p("/layers/1")]).unwrap();
        assert_eq!(
            names_at(&document, "/layers"),
            ["a", "b", "b_2", "c", "c_2", "d"]
        );
        assert_eq!(copies, [p("/layers/2"), p("/layers/4")]);
        // A copied group keeps its children, and its text as written.
        let copies = duplicate(&mut document, &[p("/layers/0")]).unwrap();
        assert_eq!(copies, [p("/layers/1")]);
        assert_eq!(names_at(&document, "/layers/1/children"), ["a1", "a2"]);
        assert!(document.text().contains(
            "    { \"name\": \"a_2\", \"type\": \"group\", \"children\": [\n      { \"name\": \"a1\""
        ), "{}", document.text());
        assert!(document.undo());
        assert!(document.undo());
        assert_eq!(document.text(), show().text());
    }

    #[test]
    fn layers_move_up_and_down_their_list() {
        let mut document = show();
        let moved = reorder(&mut document, &[p("/layers/2"), p("/layers/3")], true).unwrap();
        assert_eq!(names_at(&document, "/layers"), ["a", "c", "d", "b"]);
        assert_eq!(moved, [p("/layers/1"), p("/layers/2")]);
        let moved = reorder(&mut document, &[p("/layers/0")], false).unwrap();
        assert_eq!(names_at(&document, "/layers"), ["c", "a", "d", "b"]);
        assert_eq!(moved, [p("/layers/1")]);
        assert!(reorder(&mut document, &[p("/layers/0")], true).is_err());
        assert!(reorder(&mut document, &[p("/layers/3")], false).is_err());
        assert!(
            reorder(
                &mut document,
                &[p("/layers/3"), p("/layers/1/children/0")],
                true
            )
            .is_err(),
            "only side by side"
        );
        assert!(document.undo());
        assert!(document.undo());
        assert_eq!(document.text(), show().text());
    }

    #[test]
    fn siblings_are_grouped_where_the_first_was_and_ungrouped_back() {
        let mut document = show();
        let group = group(&mut document, &[p("/layers/3"), p("/layers/1")]).unwrap();
        assert_eq!(group, p("/layers/1"));
        assert_eq!(names_at(&document, "/layers"), ["a", "group", "c"]);
        assert_eq!(names_at(&document, "/layers/1/children"), ["b", "d"]);
        let grouped = document.text();
        assert!(
            grouped.contains(
                "    {\n      \"name\": \"group\",\n      \"type\": \"group\",\n      \"children\": [\n        { \"name\": \"b\", \"type\": \"shape\" },\n        { \"name\": \"d\", \"type\": \"shape\" }\n      ]\n    },"
            ),
            "{grouped}"
        );
        let children = ungroup(&mut document, &group).unwrap();
        assert_eq!(children, [p("/layers/1"), p("/layers/2")]);
        assert_eq!(names_at(&document, "/layers"), ["a", "b", "d", "c"]);
        assert!(document.undo(), "ungrouping is one step");
        assert_eq!(document.text(), grouped);
        assert!(document.undo(), "grouping is one step");
        assert_eq!(document.text(), show().text());
    }

    #[test]
    fn a_group_with_anything_of_its_own_stays_together() {
        let mut document = Document::parse(
            r#"{ "layers": [ { "name": "g", "type": "group", "x": 4, "opacity": 0.5, "children": [] } ] }"#,
        )
        .unwrap();
        let refused = ungroup(&mut document, &p("/layers/0")).unwrap_err();
        assert_eq!(
            refused,
            "the group has its own x, opacity, which its children would lose"
        );
        // `b` grouped, then another `b` put beside the group.
        let mut document = show();
        let grouped = group(&mut document, &[p("/layers/1")]).unwrap();
        insert(
            &mut document,
            &p("/layers"),
            9,
            json!({"name": "b", "type": "shape"}),
        )
        .unwrap();
        assert_eq!(
            ungroup(&mut document, &grouped).unwrap_err(),
            "a layer named b is beside the group already"
        );
        move_into(
            &mut document,
            &[p("/layers/1/children/0")],
            &p("/scenes/0/layers"),
        )
        .unwrap_err();
        assert_eq!(delete(&mut document, &[p("/layers/4")]), Ok(()));
        move_into(&mut document, &[p("/layers/1/children/0")], &p("/layers")).unwrap();
        assert_eq!(
            names_at(&document, "/layers"),
            ["a", "group", "c", "d", "b"]
        );
        assert_eq!(
            ungroup(&mut document, &p("/layers/1")),
            Ok(Vec::new()),
            "an empty group just goes"
        );
        assert_eq!(names_at(&document, "/layers"), ["a", "c", "d", "b"]);
    }

    #[test]
    fn layers_move_between_groups_the_show_and_scenes() {
        let mut document = show();
        // Into a group: to the end of its children.
        let moved = move_into(
            &mut document,
            &[p("/layers/2"), p("/layers/3")],
            &p("/layers/0/children"),
        )
        .unwrap();
        assert_eq!(
            moved,
            [p("/layers/0/children/2"), p("/layers/0/children/3")]
        );
        assert_eq!(
            names_at(&document, "/layers/0/children"),
            ["a1", "a2", "c", "d"]
        );
        assert!(document.undo(), "one step");
        // From a scene to the show, and from before a group into it.
        let moved = move_into(&mut document, &[p("/layers/1")], &p("/layers/0/children")).unwrap();
        assert_eq!(moved, [p("/layers/0/children/2")]);
        let moved = move_into(
            &mut document,
            &[p("/layers/0/children/0")],
            &p("/scenes/0/layers"),
        )
        .unwrap();
        assert_eq!(moved, [p("/scenes/0/layers/1")]);
        assert_eq!(names_at(&document, "/scenes/0/layers"), ["b", "a1"]);
        let mut document = show();
        let moved = move_into(&mut document, &[p("/layers/2")], &p("/layers/3/children"));
        assert!(moved.is_err(), "a shape has no children");
        assert_eq!(
            document.text(),
            show().text(),
            "a failed move leaves nothing"
        );
        assert_eq!(
            move_into(&mut document, &[p("/layers/1")], &p("/scenes/0/layers")).unwrap_err(),
            "two layers named b would be side by side"
        );
        assert_eq!(
            move_into(&mut document, &[p("/layers/0")], &p("/layers/0/children")).unwrap_err(),
            "a group cannot go inside itself"
        );
        // Out of a group, to the show's end; and before the group.
        let moved = move_into(&mut document, &[p("/layers/0/children/0")], &p("/layers")).unwrap();
        assert_eq!(moved, [p("/layers/4")]);
        // A group after the layer moved is one nearer once it is out.
        let g = group(&mut document, &[p("/layers/3")]).unwrap();
        assert_eq!(g, p("/layers/3"));
        let moved = move_into(&mut document, &[p("/layers/1")], &p("/layers/3/children")).unwrap();
        assert_eq!(moved, [p("/layers/2/children/1")]);
        assert_eq!(names_at(&document, "/layers/2/children"), ["d", "b"]);
    }
}
