//! Opening a show in each of its forms gives the same summary.

use std::path::Path;

use cuelight_editor::opened::Opened;

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
fn a_pack_opens_to_the_same_summary() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("mini.cuelight");
    cuelight_loader::pack(fixture(), &pack).unwrap();
    let from_folder = Opened::from_path(fixture()).unwrap().summary;
    let from_path = Opened::from_path(&pack).unwrap().summary;
    let from_bytes = Opened::from_bytes("mini.cuelight", &std::fs::read(&pack).unwrap())
        .unwrap()
        .summary;
    assert_eq!(from_folder, from_path);
    assert_eq!(from_folder, from_bytes);
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
