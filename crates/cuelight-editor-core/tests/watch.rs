//! A change on disk is told from the editor's own save, and says which
//! files it changed.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use std::path::{Path, PathBuf};

use cuelight_editor_core::document::Pointer;
use cuelight_editor_core::opened::Opened;
use cuelight_editor_core::save::{self, Origin};
use cuelight_editor_core::watch;
use serde_json::json;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
}

/// The mini fixture copied into a fresh folder.
fn copy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["show.json", "test-driver.json", "assets/dot.png"] {
        let to = dir.path().join(name);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixture().join(name), to).unwrap();
    }
    dir
}

#[test]
fn the_editors_own_save_is_no_change() {
    let dir = copy();
    let mut opened = Opened::from_path(dir.path()).unwrap();
    assert_eq!(watch::change(&opened.origin, &opened.files).unwrap(), None);
    opened
        .document
        .set(&Pointer::parse("/size/0").unwrap(), json!(65))
        .unwrap();
    save::save(&opened.origin, &mut opened.files, &mut opened.document).unwrap();
    assert_eq!(watch::change(&opened.origin, &opened.files).unwrap(), None);
}

#[test]
fn a_change_from_outside_says_which_files_it_changed() {
    let dir = copy();
    let opened = Opened::from_path(dir.path()).unwrap();
    let show = dir.path().join("show.json");
    let text = std::fs::read_to_string(&show)
        .unwrap()
        .replace("\"mini\"", "\"mini2\"");
    std::fs::write(&show, &text).unwrap();
    let change = watch::change(&opened.origin, &opened.files)
        .unwrap()
        .unwrap();
    assert!(change.document);
    assert!(change.others.is_empty());
    assert_eq!(change.files["show.json"], text.as_bytes());

    std::fs::write(dir.path().join("assets/dot.png"), b"not a png").unwrap();
    let change = watch::change(&opened.origin, &opened.files)
        .unwrap()
        .unwrap();
    assert_eq!(change.others, ["assets/dot.png"]);
}

#[test]
fn only_the_shows_own_files_concern_it() {
    let dir = Path::new("/shows/mini");
    let folder = Origin::Folder(dir.to_owned());
    assert_eq!(watch::watched(&folder), Some((dir, true)));
    assert!(watch::concerns(&folder, &dir.join("show.json")));
    assert!(watch::concerns(&folder, &dir.join("assets/dot.png")));
    assert!(
        !watch::concerns(&folder, &dir.join(".show.json.saving")),
        "the save's own"
    );
    assert!(!watch::concerns(&folder, &dir.join(".git/index")));
    assert!(!watch::concerns(
        &folder,
        Path::new("/shows/other/show.json")
    ));

    let pack = Origin::Pack(dir.join("mini.cuelight"));
    assert_eq!(watch::watched(&pack), Some((dir, false)));
    assert!(watch::concerns(&pack, &dir.join("mini.cuelight")));
    assert!(!watch::concerns(&pack, &dir.join("other.cuelight")));
    assert_eq!(
        watch::watched(&Origin::Bytes {
            name: "x.cuelight".to_owned()
        }),
        None
    );
}
