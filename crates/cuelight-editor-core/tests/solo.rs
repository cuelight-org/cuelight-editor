//! A layer soloed: its show holds the subtree alone, in its place, loads
//! with the show's assets, and answers its own triggers.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use std::path::{Path, PathBuf};

use cuelight_core::{LayerPath, Property, Root, Show, Value};
use cuelight_editor_core::opened::Opened;
use cuelight_editor_core::session::{Instant, lock};
use cuelight_editor_core::solo::{self, Solo};
use cuelight_editor_core::tree::{self, Row};

fn mini() -> Opened {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini");
    Opened::from_path(&dir).unwrap()
}

fn solo_of(opened: &Opened, path: &LayerPath) -> Solo {
    let show = opened.engine.show().unwrap();
    Solo::open(&opened.files, &opened.document.value(), show, path, &[]).unwrap()
}

fn number(solo: &Solo, layer: &str, property: Property) -> Option<f64> {
    lock(&solo.session.engine)
        .values()
        .unwrap()
        .into_iter()
        .find(|v| v.name == layer && v.property == property)
        .and_then(|v| match v.value {
            Value::Number(n) => Some(n),
            _ => None,
        })
}

#[test]
fn a_solo_holds_the_subtree_alone_in_its_place() {
    let opened = mini();
    let dot = LayerPath::new(Root::Show, [1, 0]);
    let mut solo = solo_of(&opened, &dot);
    assert_eq!(solo.place, "group/dot");
    assert_eq!(solo.inner, LayerPath::new(Root::Show, [0, 0]));

    let layers = solo.document["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 1, "the floor is not in it");
    let group = &layers[0];
    assert_eq!(group["name"], "group");
    assert_eq!(
        (group["x"].clone(), group["y"].clone()),
        (32.into(), 14.into())
    );
    assert_eq!(group["children"][0]["name"], "dot");
    assert_eq!(solo.document["size"], serde_json::json!([64, 32]));
    assert_eq!(solo.document["variables"]["lit"], false);

    // It loads with the show's image, and its trigger is listed.
    let engine = lock(&solo.session.engine);
    let show = engine.show().unwrap();
    let names: Vec<String> = tree::rows(show)
        .into_iter()
        .filter_map(|row| match row {
            Row::Layer { name, .. } => Some(name),
            Row::Root { .. } => None,
        })
        .collect();
    assert_eq!(names, ["group", "dot"]);
    assert!(engine.bounds(&solo.inner).is_some(), "the dot is drawn");
    drop(engine);
    assert_eq!(solo.triggers, ["go"]);
    let [x, y, w, h] = solo.area().unwrap();
    assert!(
        x < 32.0 && x + w > 32.0 && y < 14.0 && y + h > 14.0,
        "{x} {y} {w} {h}"
    );

    // Its own clock: fired at 0, the dot is up a little later.
    solo.session.fire("go");
    solo.session.step(0.2, Instant::now());
    let up = number(&solo, "dot", Property::Y).unwrap();
    assert!(up < -5.0, "{up}");
}

#[test]
fn a_solo_follows_the_show_as_edited_and_says_when_its_layer_is_gone() {
    let opened = mini();
    let dot = LayerPath::new(Root::Show, [1, 0]);
    let mut solo = solo_of(&opened, &dot);
    let mut document = opened.document.value();
    document["layers"][1]["children"][0]["x"] = 5.into();
    solo.reload(&document, Instant::now()).unwrap();
    assert_eq!(solo.document["layers"][0]["children"][0]["x"], 5);
    assert_eq!(number(&solo, "dot", Property::X), Some(5.0));

    document["layers"][1]["children"] = serde_json::json!([]);
    assert!(solo.reload(&document, Instant::now()).is_err());
}

#[test]
fn a_part_solos_its_artwork() {
    let show: Show = serde_json::from_str(
        r##"{ "format": 1, "name": "t", "size": [8, 8], "layers": [
          { "name": "wolf", "type": "image", "image": "wolf", "parts": [ { "id": "jaw" } ] } ] }"##,
    )
    .unwrap();
    assert_eq!(
        solo::soloed(&show, &LayerPath::new(Root::Show, [0, 0])),
        Some(LayerPath::new(Root::Show, [0]))
    );
}

/// The examples checkout's picture book, beside this repository or
/// where `CUELIGHT_EXAMPLES` says.
fn picture_book() -> Option<PathBuf> {
    let dir = std::env::var_os("CUELIGHT_EXAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../cuelight-examples")
        });
    let book = dir.join("demos/red_riding_hood");
    book.join("show.json").exists().then_some(book)
}

/// The first layer named `name` in the tree with a timeline `trigger`
/// starts.
fn layer_heard(show: &Show, name: &str, trigger: &str) -> Option<LayerPath> {
    tree::rows(show).into_iter().find_map(|row| match row {
        Row::Layer { path, name: n, .. }
            if n == name
                && tree::layer(show, &path).is_some_and(|l| {
                    l.timelines
                        .iter()
                        .any(|t| t.trigger.iter().any(|t| t == trigger))
                }) =>
        {
            Some(path)
        }
        _ => None,
    })
}

#[test]
fn the_wolf_of_the_picture_book_solos_and_hops() {
    let Some(book) = picture_book() else {
        eprintln!("no cuelight-examples checkout: the wolf is not soloed");
        return;
    };
    let opened = Opened::from_path(&book).unwrap();
    let show = opened.engine.show().unwrap();
    let wolf = layer_heard(show, "wolf", "tap_wolf").expect("a wolf that hears tap_wolf");
    let mut solo = solo_of(&opened, &wolf);
    assert_eq!(solo.triggers, ["tap_wolf"]);
    assert!(solo.document.get("scenes").is_none());
    let before = solo.area().expect("the wolf is drawn");
    // Within the picture, right of the page's middle.
    assert!(before[0] > 640.0, "{before:?}");

    solo.session.fire("tap_wolf");
    solo.session.step(0.2, Instant::now());
    let after = solo.area().unwrap();
    assert!(after[1] < before[1] - 10.0, "it hops: {before:?} {after:?}");

    // Saved as a show, it takes its own image and the fonts, not the
    // other pictures.
    let files = solo.files(&opened.files, &opened.library).unwrap();
    assert!(files.keys().any(|f| f.contains("wolf")), "{files:?}");
    assert!(
        !files.keys().any(|f| f.contains("bg_woods")),
        "{:?}",
        files.keys()
    );
    let packed = cuelight_loader::pack_bytes(&files).unwrap();
    let again = Opened::from_bytes("wolf.cuelight", &packed).unwrap();
    assert!(
        again.summary.problems.is_empty(),
        "{:?}",
        again.summary.problems
    );
}
