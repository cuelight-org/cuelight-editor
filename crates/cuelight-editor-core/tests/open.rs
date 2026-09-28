//! Opening a show in each of its forms gives the same summary.

use std::path::Path;

use cuelight_editor_core::assets::{Kind, Use};
use cuelight_editor_core::opened::Opened;

fn fixture() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mini"))
}

#[test]
fn a_folder_opens_with_its_assets_and_driver() {
    let opened = Opened::from_path(fixture()).unwrap();
    let s = &opened.summary;
    assert_eq!(s.name, "mini");
    assert_eq!(s.size, [64, 32]);
    assert_eq!(s.layers, 3, "the group's child counts");
    assert_eq!((s.variables, s.keys, s.pressable), (1, 1, 1));
    assert_eq!(
        (s.images, s.vectors, s.fonts, s.sounds, s.videos),
        (1, 0, 0, 0, 0)
    );
    assert_eq!(s.driver, Some((5, true)));
    assert!(s.problems.is_empty(), "{:?}", s.problems);
}

#[test]
fn a_folder_lists_its_assets_and_where_they_are_used() {
    let opened = Opened::from_path(fixture()).unwrap();
    assert_eq!(
        opened.files.keys().collect::<Vec<_>>(),
        ["assets/dot.png", "show.json", "test-driver.json"]
    );
    let [dot] = opened.library.as_slice() else {
        panic!("one asset: {:?}", opened.library);
    };
    assert_eq!((dot.name.as_str(), dot.kind), ("dot", Kind::Image));
    assert_eq!(dot.file.as_deref(), Some("assets/dot.png"));
    assert_eq!(dot.size, Some([8.0, 8.0]));
    assert_eq!(
        dot.uses,
        [Use {
            place: "group/dot".to_owned(),
            how: "image layer".to_owned()
        }]
    );
}

#[test]
fn a_pack_opens_to_the_same_summary_and_library() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("mini.cuelight");
    cuelight_loader::pack(fixture(), &pack).unwrap();
    let from_folder = Opened::from_path(fixture()).unwrap();
    let from_path = Opened::from_path(&pack).unwrap();
    let from_bytes = Opened::from_bytes("mini.cuelight", &std::fs::read(&pack).unwrap()).unwrap();
    assert_eq!(from_folder.summary, from_path.summary);
    assert_eq!(from_folder.summary, from_bytes.summary);
    assert_eq!(from_folder.library, from_path.library);
    assert_eq!(from_folder.library, from_bytes.library);
    assert_eq!(from_folder.files, from_bytes.files);
}

#[test]
fn a_loose_document_opens_without_its_assets() {
    let bytes = std::fs::read(fixture().join("show.json")).unwrap();
    let opened = Opened::from_bytes("show.json", &bytes).unwrap();
    assert_eq!(opened.summary.name, "mini");
    assert_eq!(
        opened.summary.images, 0,
        "a loose document brings no assets"
    );
}

#[test]
fn an_unfinished_show_opens_with_what_it_dropped() {
    let opened = Opened::from_bytes(
        "show.json",
        br##"{ "format": 1, "name": "x", "size": [8, 8], "layers": [
              { "name": "ok", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "fill": "#FFFFFF" },
              { "name": "odd", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "x": "far" }
            ] }"##,
    )
    .unwrap();
    assert_eq!(opened.summary.layers, 1, "the layer that parses is kept");
    assert_eq!(
        opened.summary.problems.len(),
        1,
        "{:?}",
        opened.summary.problems
    );
    assert!(
        opened.summary.problems[0].starts_with("layers[1]"),
        "{:?}",
        opened.summary.problems
    );
}

#[test]
fn a_newer_format_is_refused_with_its_number() {
    let error = Opened::from_bytes(
        "show.json",
        br#"{ "format": 99, "name": "x", "size": [1, 1], "layers": [] }"#,
    )
    .err()
    .expect("an error")
    .to_string();
    assert!(error.contains("format 99"), "{error}");
}

#[test]
fn a_folder_without_a_show_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let error = Opened::from_path(dir.path())
        .err()
        .expect("an error")
        .to_string();
    assert!(error.contains("show.json"), "{error}");
}
