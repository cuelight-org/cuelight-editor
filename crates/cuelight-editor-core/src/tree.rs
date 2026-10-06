//! The show's layers as the tree lists them: the show's own layers, then
//! each scene's, in document order, with groups' children and an artwork
//! layer's parts under them; and the lookups the window makes by a
//! layer's path.

use cuelight_core::{
    DigitDisplay, Layer, LayerKind, LayerPath, Property, Root, Show, layer_at, root_layers,
};

/// One row of the tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// A heading: the show's own layers, or a scene by index and name.
    Root { root: Root, name: String },
    /// A layer, at its depth under the heading (0 for a top-level one).
    Layer {
        path: LayerPath,
        name: String,
        kind: &'static str,
        depth: usize,
    },
}

/// Every row of the tree, in the order it is shown.
pub fn rows(show: &Show) -> Vec<Row> {
    let mut out = Vec::new();
    out.push(Row::Root {
        root: Root::Show,
        name: "show".to_owned(),
    });
    walk(&show.layers, Root::Show, &mut Vec::new(), &mut out);
    for (i, scene) in show.scenes.iter().enumerate() {
        out.push(Row::Root {
            root: Root::Scene(i),
            name: scene.name.clone(),
        });
        walk(&scene.layers, Root::Scene(i), &mut Vec::new(), &mut out);
    }
    out
}

fn walk(layers: &[Layer], root: Root, indices: &mut Vec<usize>, out: &mut Vec<Row>) {
    for (i, layer) in layers.iter().enumerate() {
        indices.push(i);
        out.push(Row::Layer {
            path: LayerPath::new(root, indices.clone()),
            name: layer.name.clone(),
            kind: kind_name(&layer.kind),
            depth: indices.len() - 1,
        });
        walk(layer.children(), root, indices, out);
        indices.pop();
    }
}

/// The kind of a layer, as the tree tags it.
pub fn kind_name(kind: &LayerKind) -> &'static str {
    match kind {
        LayerKind::Group { .. } => "group",
        LayerKind::Shape { .. } => "shape",
        LayerKind::Image { .. } => "image",
        LayerKind::Text { .. } => "text",
        LayerKind::Digits {
            display: DigitDisplay::Reel(_),
            ..
        } => "reels",
        LayerKind::Digits { .. } => "digits",
        LayerKind::Audio { .. } => "audio",
        LayerKind::Video { .. } => "video",
        LayerKind::Part { .. } => "part",
    }
}

/// The layer at `path`, if the show has one there.
pub fn layer<'a>(show: &'a Show, path: &LayerPath) -> Option<&'a Layer> {
    layer_at(root_layers(show, path.root)?, &path.indices)
}

/// Where a path leads, in words: `group/dot`, or `scene play: out_tide`.
pub fn describe(show: &Show, path: &LayerPath) -> String {
    let mut names = Vec::new();
    let mut layers = root_layers(show, path.root).unwrap_or(&[]);
    for &i in &path.indices {
        let Some(layer) = layers.get(i) else {
            names.push("?".to_owned());
            break;
        };
        names.push(layer.name.clone());
        layers = layer.children();
    }
    let place = names.join("/");
    match path.root {
        Root::Show => place,
        Root::Scene(i) => match show.scenes.get(i) {
            Some(scene) => format!("scene {}: {place}", scene.name),
            None => format!("scene {i}: {place}"),
        },
    }
}

/// Where `path` is in the document, as a JSON pointer: `/layers/1/children/0`
/// for a group's child, `/layers/1/parts/0` for an artwork layer's part,
/// `/scenes/0/layers/2` in a scene. `None` when the show has no layer
/// there.
pub fn pointer(show: &Show, path: &LayerPath) -> Option<String> {
    let mut out = match path.root {
        Root::Show => "/layers".to_owned(),
        Root::Scene(i) => format!("/scenes/{i}/layers"),
    };
    let mut layers = root_layers(show, path.root)?;
    for (depth, &i) in path.indices.iter().enumerate() {
        let layer = layers.get(i)?;
        out.push_str(&format!("/{i}"));
        if depth + 1 < path.indices.len() {
            out.push_str(if layer.holds_parts() {
                "/parts"
            } else {
                "/children"
            });
        }
        layers = layer.children();
    }
    Some(out)
}

/// Whether `path` is `ancestor` or lies under it.
pub fn within(path: &LayerPath, ancestor: &LayerPath) -> bool {
    path.root == ancestor.root && path.indices.starts_with(&ancestor.indices)
}

/// A property's name as the show writes it: `scale_x`.
pub fn property_name(property: Property) -> String {
    serde_json::to_value(property)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{property:?}"))
}

/// Whether `property` does anything for a layer of this kind. Every
/// layer can be given a place, a scale and an opacity, but a sound
/// draws nothing: it is heard or not (`visible`), at its gain. A part of
/// an artwork has only its placement, opacity and visibility.
pub fn applies(property: Property, kind: &LayerKind) -> bool {
    match kind {
        LayerKind::Audio { .. } => {
            matches!(
                property,
                Property::Visible | Property::Gain | Property::Sound
            )
        }
        LayerKind::Part { .. } => matches!(
            property,
            Property::X
                | Property::Y
                | Property::Rotation
                | Property::Scale
                | Property::ScaleX
                | Property::ScaleY
                | Property::Opacity
                | Property::Visible
        ),
        _ => true,
    }
}

/// Every property a layer might have, in the order the inspector
/// lists them: placement first, then appearance, then what the kind adds.
pub const PROPERTIES: [Property; 18] = [
    Property::X,
    Property::Y,
    Property::Rotation,
    Property::Scale,
    Property::ScaleX,
    Property::ScaleY,
    Property::Opacity,
    Property::Visible,
    Property::Tint,
    Property::Text,
    Property::Font,
    Property::Reveal,
    Property::Frame,
    Property::TileX,
    Property::TileY,
    Property::Gain,
    Property::Sound,
    Property::Video,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn show() -> Show {
        serde_json::from_str(
            r##"{ "format": 1, "name": "t", "size": [8, 8], "layers": [
              { "name": "floor", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "fill": "#FFFFFF" },
              { "name": "group", "type": "group", "children": [
                { "name": "dot", "type": "shape", "shape": { "circle": [1, 1, 1] }, "fill": "#FFFFFF" } ] } ],
              "scenes": [ { "name": "play", "trigger": "play", "layers": [
                { "name": "sign", "type": "shape", "shape": { "rect": [0, 0, 2, 2] }, "fill": "#FFFFFF" } ] } ] }"##,
        )
        .unwrap()
    }

    #[test]
    fn the_tree_lists_the_show_then_each_scene() {
        let show = show();
        let rows = rows(&show);
        let names: Vec<String> = rows
            .iter()
            .map(|r| match r {
                Row::Root { name, .. } => format!("[{name}]"),
                Row::Layer { name, depth, .. } => format!("{}{name}", "  ".repeat(*depth)),
            })
            .collect();
        assert_eq!(
            names,
            ["[show]", "floor", "group", "  dot", "[play]", "sign"]
        );
        let Row::Layer { path, kind, .. } = &rows[3] else {
            panic!("a layer");
        };
        assert_eq!(*path, LayerPath::new(Root::Show, [1, 0]));
        assert_eq!(*kind, "shape");
        assert_eq!(describe(&show, path), "group/dot");
        assert_eq!(
            describe(&show, &LayerPath::new(Root::Scene(0), [0])),
            "scene play: sign"
        );
        assert_eq!(layer(&show, path).map(|l| l.name.as_str()), Some("dot"));
        assert!(within(path, &LayerPath::new(Root::Show, [1])));
        assert!(!within(path, &LayerPath::new(Root::Show, [0])));
        assert_eq!(property_name(Property::ScaleX), "scale_x");
        assert_eq!(
            pointer(&show, path).as_deref(),
            Some("/layers/1/children/0")
        );
        assert_eq!(
            pointer(&show, &LayerPath::new(Root::Scene(0), [0])).as_deref(),
            Some("/scenes/0/layers/0")
        );
        assert_eq!(pointer(&show, &LayerPath::new(Root::Show, [1, 7])), None);
    }

    /// An artwork layer's parts are layers under it, and live under
    /// `parts` in the document where a group's children live under
    /// `children`.
    #[test]
    fn an_artworks_parts_are_listed_under_it_and_addressed_through_parts() {
        let show: Show = serde_json::from_str(
            r##"{ "format": 1, "name": "t", "size": [8, 8], "layers": [
              { "name": "wolf", "type": "image", "image": "wolf", "parts": [
                { "id": "jaw", "pivot": [4, 4] },
                { "id": "tail" } ] } ] }"##,
        )
        .unwrap();
        let rows = rows(&show);
        let names: Vec<String> = rows
            .iter()
            .map(|r| match r {
                Row::Root { name, .. } => format!("[{name}]"),
                Row::Layer {
                    name, depth, kind, ..
                } => format!("{}{name} ({kind})", "  ".repeat(*depth)),
            })
            .collect();
        assert_eq!(
            names,
            ["[show]", "wolf (image)", "  jaw (part)", "  tail (part)"]
        );
        let jaw = LayerPath::new(Root::Show, [0, 0]);
        assert_eq!(describe(&show, &jaw), "wolf/jaw");
        assert_eq!(pointer(&show, &jaw).as_deref(), Some("/layers/0/parts/0"));
        assert_eq!(layer(&show, &jaw).map(|l| l.name.as_str()), Some("jaw"));
        assert!(matches!(
            layer(&show, &jaw).map(|l| &l.kind),
            Some(LayerKind::Part { id, pivot: Some([4.0, 4.0]) }) if id == "jaw"
        ));
    }
}
