//! What a piece of vector artwork is made of, as the library shows it:
//! the element ids its paths carry, nested as the SVG nests them, and
//! which of them the show's layers move as parts.

use std::collections::BTreeMap;

use cuelight::Vector;
use cuelight_core::{Layer, LayerKind, Show};

/// One element id of the artwork, at its depth in the id tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub id: String,
    /// 0 for an id no other id holds.
    pub depth: usize,
    /// The paths inside it, its own included.
    pub paths: usize,
    /// The layers that name it as a part, by their place in the show.
    pub parts: Vec<String>,
}

/// The artwork's structure: its ids as a tree, and what the show asks
/// of it that it does not have.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Structure {
    /// Every id, parents before their children, in paint order.
    pub elements: Vec<Element>,
    /// All the artwork's paths, and those inside no id.
    pub paths: usize,
    pub loose: usize,
    /// Parts the show names that no element of the artwork carries,
    /// with the layer that names each.
    pub unknown: Vec<(String, String)>,
}

/// The structure of the artwork registered as `name`, with the parts the
/// show's artwork layers name for it.
pub fn structure(vector: &Vector, name: &str, show: Option<&Show>) -> Structure {
    let mut named = BTreeMap::<String, Vec<String>>::new();
    if let Some(show) = show {
        parts_in(&show.layers, "", name, &mut named);
        for scene in &show.scenes {
            parts_in(
                &scene.layers,
                &format!("scene {}: ", scene.name),
                name,
                &mut named,
            );
        }
    }

    // The tree as nodes with their parent, found by walking each path's
    // ids from the outermost in.
    struct Node {
        id: String,
        parent: Option<usize>,
        paths: usize,
    }
    let mut nodes: Vec<Node> = Vec::new();
    let mut loose = 0;
    for path in &vector.paths {
        if path.ids.is_empty() {
            loose += 1;
        }
        let mut parent = None;
        for id in &path.ids {
            let at = match nodes.iter().position(|n| n.parent == parent && n.id == *id) {
                Some(at) => at,
                None => {
                    nodes.push(Node {
                        id: id.clone(),
                        parent,
                        paths: 0,
                    });
                    nodes.len() - 1
                }
            };
            nodes[at].paths += 1;
            parent = Some(at);
        }
    }

    // Depth first, children in the order they were first painted.
    fn visit(
        nodes: &[Node],
        parent: Option<usize>,
        depth: usize,
        named: &mut BTreeMap<String, Vec<String>>,
        out: &mut Vec<Element>,
    ) {
        for (i, node) in nodes.iter().enumerate() {
            if node.parent != parent {
                continue;
            }
            out.push(Element {
                id: node.id.clone(),
                depth,
                paths: node.paths,
                parts: named.get(&node.id).cloned().unwrap_or_default(),
            });
            visit(nodes, Some(i), depth + 1, named, out);
        }
    }
    let mut elements = Vec::new();
    visit(&nodes, None, 0, &mut named, &mut elements);

    let unknown = named
        .into_iter()
        .filter(|(id, _)| !nodes.iter().any(|n| n.id == *id))
        .flat_map(|(id, places)| places.into_iter().map(move |p| (id.clone(), p)))
        .collect();
    Structure {
        elements,
        paths: vector.paths.len(),
        loose,
        unknown,
    }
}

/// The part ids that artwork layers drawing `artwork` name, each with
/// the places of the layers that name it.
fn parts_in(
    layers: &[Layer],
    prefix: &str,
    artwork: &str,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    for layer in layers {
        let place = if prefix.is_empty() || prefix.ends_with(": ") {
            format!("{prefix}{}", layer.name)
        } else {
            format!("{prefix}/{}", layer.name)
        };
        if let LayerKind::Image { image, parts, .. } = &layer.kind
            && image == artwork
        {
            for part in parts {
                if let LayerKind::Part { id, .. } = &part.kind {
                    let places = out.entry(id.clone()).or_default();
                    if !places.contains(&place) {
                        places.push(place.clone());
                    }
                }
            }
        }
        if !layer.holds_parts() {
            parts_in(layer.children(), &place, artwork, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuelight::VectorPath;

    fn path(ids: &[&str]) -> VectorPath {
        VectorPath {
            elements: Vec::new(),
            fill: Some([0, 0, 0, 255]),
            stroke: None,
            ids: ids.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn robot() -> Vector {
        Vector {
            width: 100.0,
            height: 100.0,
            paths: vec![
                path(&[]),
                path(&["body"]),
                path(&["head", "eyes", "eye_left"]),
                path(&["head", "eyes", "eye_right"]),
                path(&["head", "jaw"]),
                path(&["head"]),
                path(&["arm"]),
            ],
        }
    }

    fn rows(structure: &Structure) -> Vec<(usize, &str, usize)> {
        structure
            .elements
            .iter()
            .map(|e| (e.depth, e.id.as_str(), e.paths))
            .collect()
    }

    #[test]
    fn ids_nest_as_the_artwork_nests_them() {
        let structure = structure(&robot(), "robot", None);
        assert_eq!(
            rows(&structure),
            [
                (0, "body", 1),
                (0, "head", 4),
                (1, "eyes", 2),
                (2, "eye_left", 1),
                (2, "eye_right", 1),
                (1, "jaw", 1),
                (0, "arm", 1),
            ]
        );
        assert_eq!(structure.paths, 7);
        assert_eq!(structure.loose, 1);
        assert!(structure.elements.iter().all(|e| e.parts.is_empty()));
    }

    #[test]
    fn an_id_met_again_later_is_one_element() {
        let vector = Vector {
            width: 1.0,
            height: 1.0,
            paths: vec![path(&["a", "b"]), path(&["c"]), path(&["a"])],
        };
        assert_eq!(
            rows(&structure(&vector, "v", None)),
            [(0, "a", 2), (1, "b", 1), (0, "c", 1)]
        );
    }

    #[test]
    fn the_parts_the_show_names_are_marked() {
        let show: Show = serde_json::from_value(serde_json::json!({
            "name": "parts",
            "size": [100, 100],
            "layers": [
                { "name": "still", "type": "image", "image": "robot" },
                { "name": "wave", "type": "image", "image": "robot",
                  "parts": [ { "id": "arm" }, { "id": "tail" } ] },
                { "name": "other", "type": "image", "image": "cat",
                  "parts": [ { "id": "head" } ] },
                { "name": "g", "type": "group", "children": [
                    { "name": "talk", "type": "image", "image": "robot",
                      "parts": [ { "id": "jaw" }, { "id": "arm" } ] }
                ] }
            ],
            "scenes": [
                { "name": "end", "layers": [
                    { "name": "blink", "type": "image", "image": "robot",
                      "parts": [ { "id": "eyes" } ] }
                ] }
            ]
        }))
        .unwrap();
        let structure = structure(&robot(), "robot", Some(&show));
        let parts: Vec<(&str, Vec<&str>)> = structure
            .elements
            .iter()
            .filter(|e| !e.parts.is_empty())
            .map(|e| (e.id.as_str(), e.parts.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(
            parts,
            [
                ("eyes", vec!["scene end: blink"]),
                ("jaw", vec!["g/talk"]),
                ("arm", vec!["wave", "g/talk"]),
            ]
        );
        assert_eq!(
            structure.unknown,
            [("tail".to_owned(), "wave".to_owned())],
            "a part the artwork has no element for"
        );
    }
}
