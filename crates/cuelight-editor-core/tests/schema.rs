//! The schema walk: every key the show format gives a layer is either
//! edited by the inspector, as a property or a field, or named as left to
//! another part of the editor; and every field the inspector edits, set
//! on a layer of each kind that has it, is what the engine then reads.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use std::collections::{BTreeMap, BTreeSet};

use cuelight::Engine;
use cuelight_editor_core::document::{Document, Pointer};
use cuelight_editor_core::fields::{self, FIELDS, Input};
use cuelight_editor_core::tree;
use serde_json::{Value, json};

/// A schema node with its `$ref`s followed.
fn resolve(defs: &Value, node: &Value) -> Value {
    let mut node = node;
    while let Some(name) = node.get("$ref").and_then(Value::as_str) {
        node = &defs[name.rsplit('/').next().unwrap()];
    }
    node.clone()
}

/// The layer keys of the format: the shared ones under `""`, then each
/// kind's own under its `type`.
fn layer_keys() -> BTreeMap<String, BTreeSet<String>> {
    let schema = serde_json::to_value(schemars::schema_for!(cuelight_core::Show)).unwrap();
    let defs = &schema["$defs"];
    let resolve = |node: &Value| resolve(defs, node);
    let layer = resolve(&defs["Layer"]);
    let keys = |node: &Value| -> BTreeSet<String> {
        node["properties"]
            .as_object()
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default()
    };
    let mut out = BTreeMap::from([(String::new(), keys(&layer))]);
    for variant in layer["oneOf"].as_array().unwrap() {
        let variant = resolve(variant);
        let kind = variant["properties"]["type"]["const"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut own = keys(&variant);
        own.remove("type");
        out.insert(kind, own);
    }
    out
}

#[test]
fn every_layer_key_is_edited_or_left_to_a_named_part() {
    let properties: BTreeSet<String> = tree::PROPERTIES
        .iter()
        .map(|p| tree::property_name(*p))
        .collect();
    let edited: BTreeSet<&str> = FIELDS
        .iter()
        .filter_map(|f| f.path.first().copied())
        .collect();
    let elsewhere: BTreeSet<&str> = fields::ELSEWHERE.iter().map(|(key, _)| *key).collect();
    let mut missing = Vec::new();
    for (kind, keys) in layer_keys() {
        for key in keys {
            let tile = key == "repeat" && kind == "image"; // the offset is a property
            if !(properties.contains(&key)
                || edited.contains(key.as_str())
                || elsewhere.contains(key.as_str())
                || tile)
            {
                missing.push(format!("{kind:?}: {key}"));
            }
        }
    }
    assert!(missing.is_empty(), "keys nobody edits: {missing:?}");
}

#[test]
fn every_field_is_a_key_of_the_kinds_it_claims() {
    let keys = layer_keys();
    for field in FIELDS {
        let key = field.path.first().unwrap().to_string();
        if field.kinds.is_empty() {
            assert!(
                keys[""].contains(&key),
                "{} is not shared by every layer",
                field.label
            );
        }
        for kind in field.kinds {
            // A vector layer is an image layer drawn from SVG artwork.
            let schema_kind = if *kind == "vector" { "image" } else { kind };
            let own = &keys[schema_kind];
            assert!(
                own.contains(&key) || keys[""].contains(&key) || *kind == "vector",
                "{kind} has no {key}"
            );
        }
    }
}

/// A layer of each kind, as small as loads.
fn sample(kind: &str) -> Value {
    match kind {
        "shape" => {
            json!({"name": "l", "type": "shape", "shape": {"rect": [0, 0, 10, 10]}, "fill": "#FFFFFF"})
        }
        "text" => json!({"name": "l", "type": "text", "font": "f", "text": "AB"}),
        "digits" => {
            json!({"name": "l", "type": "digits", "digits": 3, "size": [30, 10], "text": "123", "display": {"segments": {"style": "numeric7", "fill": "#FFFFFF"}}})
        }
        "image" => json!({"name": "l", "type": "image", "image": "art"}),
        "vector" => json!({"name": "l", "type": "vector", "vector": "art"}),
        "audio" => json!({"name": "l", "type": "audio", "sound": "beep"}),
        "video" => json!({"name": "l", "type": "video", "video": "clip"}),
        "part" => json!({"name": "l", "type": "image", "image": "art", "parts": [{"id": "p"}]}),
        "group" => json!({"name": "l", "type": "group", "children": []}),
        _ => panic!("no sample for {kind}"),
    }
}

/// A value the field can take that differs from the sample's own.
fn value(input: Input) -> Value {
    match input {
        Input::Choice(words) => json!(words.last().unwrap()),
        Input::Toggle => json!(true),
        Input::Number => json!(2.5),
        Input::Count => json!(2),
        Input::Pair => json!([12, 8]),
        Input::Colour => json!("#FF0000"),
        Input::Text => json!("go"),
        Input::Artwork => json!("art"),
    }
}

#[test]
fn every_field_set_is_what_the_engine_reads() {
    let kinds = [
        "shape", "text", "digits", "image", "vector", "audio", "video", "part", "group",
    ];
    let mut checked = 0;
    for kind in kinds {
        for field in fields::of(kind) {
            let show = json!({"format": 1, "name": "t", "size": [64, 32],
                "fonts": {"f": {"file": "none", "size": 8}}, "layers": [sample(kind)]});
            let mut document =
                Document::parse(&serde_json::to_string_pretty(&show).unwrap()).unwrap();
            let layer = if kind == "part" {
                Pointer::parse("/layers/0/parts/0").unwrap()
            } else {
                Pointer::parse("/layers/0").unwrap()
            };
            let set = value(field.input);
            fields::set(&mut document, &layer, field, set.clone()).unwrap();

            let mut engine = Engine::new();
            let findings = engine.load_show_tolerant(&document.text()).unwrap();
            let Some(loaded) = engine.show().unwrap().layers.first() else {
                panic!(
                    "{kind} {}: the layer did not load: {findings:?}",
                    field.label
                );
            };
            let loaded = serde_json::to_value(loaded).unwrap();
            let loaded = if kind == "part" {
                loaded["parts"][0].clone()
            } else {
                loaded
            };
            // A vector layer is read as an image layer: its artwork is
            // the engine's `image`.
            let read = if field.label == "vector" {
                loaded.get("image").filter(|v| !v.is_null())
            } else {
                fields::read(&loaded, field)
            };
            let same = match (read, &set) {
                (Some(Value::Number(a)), Value::Number(b)) => a.as_f64() == b.as_f64(),
                (Some(Value::Array(a)), Value::Array(b)) => {
                    a.iter().zip(b).all(|(x, y)| x.as_f64() == y.as_f64())
                }
                (Some(read), set) => {
                    read == set
                        || read.as_str().map(str::to_uppercase)
                            == set.as_str().map(str::to_uppercase)
                }
                (None, _) => false,
            };
            assert!(
                same,
                "{kind} {}: set {set}, the engine reads {read:?}",
                field.label
            );
            checked += 1;
        }
    }
    assert!(checked > 30, "only {checked} checked");
}
