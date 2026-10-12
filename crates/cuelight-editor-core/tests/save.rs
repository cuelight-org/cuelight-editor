//! A show saves back where it came from: untouched, byte for byte; with
//! one number changed, one line changed.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cuelight_editor_core::document::{Part, Pointer};
use cuelight_editor_core::opened::Opened;
use cuelight_editor_core::save::{self, SaveError};
use serde_json::{Value, json};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The examples checkout's show folders, beside this repository or
/// where `CUELIGHT_EXAMPLES` says; none without a checkout.
fn example_shows() -> Vec<PathBuf> {
    let dir = std::env::var_os("CUELIGHT_EXAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../cuelight-examples")
        });
    let Ok(catalog) = std::fs::read_to_string(dir.join("examples.json")) else {
        eprintln!("no cuelight-examples checkout: only the fixtures are saved");
        return Vec::new();
    };
    let catalog: Value = serde_json::from_str(&catalog).unwrap();
    catalog["categories"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["examples"].as_array().unwrap())
        .map(|e| dir.join(e["path"].as_str().unwrap()))
        .collect()
}

/// Every show to save: the fixtures, and the examples when checked out.
fn shows() -> Vec<PathBuf> {
    let mut shows = vec![fixture("mini"), fixture("typed")];
    shows.extend(example_shows());
    shows
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Every file under `dir`, by its path within it.
fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(name, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// The first number in the document, depth first, as a pointer.
fn first_number(value: &Value, at: Pointer) -> Option<(Pointer, f64)> {
    match value {
        Value::Number(n) => Some((at, n.as_f64()?)),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(i, v)| first_number(v, at.then(Part::Index(i)))),
        Value::Object(map) => map
            .iter()
            .find_map(|(k, v)| first_number(v, at.then(Part::Key(k.clone())))),
        _ => None,
    }
}

#[test]
fn every_show_saved_untouched_is_the_same_files() {
    for show in shows() {
        let dir = tempfile::tempdir().unwrap();
        copy_dir(&show, dir.path());
        let before = files_under(dir.path());
        let mut opened = Opened::from_path(dir.path()).unwrap();
        save::save(&opened.origin, &mut opened.files, &mut opened.document).unwrap();
        assert!(
            before == files_under(dir.path()),
            "{} changed on save",
            show.display()
        );
    }
}

#[test]
fn every_show_with_one_number_changed_saves_one_line_changed() {
    for show in shows() {
        let dir = tempfile::tempdir().unwrap();
        copy_dir(&show, dir.path());
        let before = std::fs::read_to_string(dir.path().join("show.json")).unwrap();
        let mut opened = Opened::from_path(dir.path()).unwrap();
        // Any number but the format's, which says how to read the rest.
        let mut value = opened.document.value();
        value.as_object_mut().unwrap().remove("format");
        let (path, n) = first_number(&value, Pointer(Vec::new())).unwrap();
        let changed = if n.fract() == 0.0 {
            json!(n as i64 + 1)
        } else {
            json!(n + 0.5)
        };
        opened.document.set(&path, changed).unwrap();
        assert!(opened.document.is_dirty());
        save::save(&opened.origin, &mut opened.files, &mut opened.document).unwrap();
        assert!(!opened.document.is_dirty());
        let after = std::fs::read_to_string(dir.path().join("show.json")).unwrap();
        let (before, after): (Vec<_>, Vec<_>) = (before.lines().collect(), after.lines().collect());
        assert_eq!(before.len(), after.len(), "{}", show.display());
        let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 1, "{} at {path}", show.display());
    }
}

#[test]
fn a_pack_is_packed_again_with_its_other_files_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("mini.cuelight");
    cuelight_loader::pack(fixture("mini"), &pack).unwrap();
    let before = cuelight_loader::read_pack(&pack).unwrap();
    let mut opened = Opened::from_path(&pack).unwrap();
    opened
        .document
        .set(&Pointer::parse("/size/0").unwrap(), json!(65))
        .unwrap();
    save::save(&opened.origin, &mut opened.files, &mut opened.document).unwrap();
    let after = cuelight_loader::read_pack(&pack).unwrap();
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (name, bytes) in &before {
        if name != "show.json" {
            assert!(bytes == &after[name], "{name} changed");
        }
    }
    assert_eq!(after["show.json"], opened.document.text().into_bytes());
    assert_eq!(Opened::from_path(&pack).unwrap().summary.size, [65, 32]);
}

#[test]
fn a_loose_document_is_written_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("loose.json");
    std::fs::copy(fixture("typed").join("show.json"), &path).unwrap();
    let mut opened = Opened::from_path(&path).unwrap();
    opened
        .document
        .set(&Pointer::parse("/name").unwrap(), json!("renamed"))
        .unwrap();
    save::save(&opened.origin, &mut opened.files, &mut opened.document).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        opened.document.text()
    );
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "nothing is left beside it"
    );
}

#[test]
fn a_show_asking_for_a_newer_format_is_not_written() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture("mini"), dir.path());
    let before = std::fs::read(dir.path().join("show.json")).unwrap();
    let mut opened = Opened::from_path(dir.path()).unwrap();
    let newer = cuelight_core::FORMAT + 1;
    opened
        .document
        .set(&Pointer::parse("/format").unwrap(), json!(newer))
        .unwrap();
    let error = save::save(&opened.origin, &mut opened.files, &mut opened.document).unwrap_err();
    assert!(
        matches!(error, SaveError::NewerFormat { found, .. } if found == newer),
        "{error}"
    );
    assert!(opened.document.is_dirty());
    assert_eq!(std::fs::read(dir.path().join("show.json")).unwrap(), before);
}

#[test]
fn a_show_that_came_as_bytes_is_handed_back_as_a_download() {
    let packed = cuelight_loader::pack_bytes(&files_under(&fixture("mini"))).unwrap();
    let mut opened = Opened::from_bytes("mini.cuelight", &packed).unwrap();
    assert!(matches!(
        save::save(&opened.origin, &mut opened.files, &mut opened.document),
        Err(SaveError::NoPlace)
    ));
    opened
        .document
        .set(&Pointer::parse("/size/1").unwrap(), json!(33))
        .unwrap();
    let (name, bytes) = save::download(&opened.origin, &opened.files, &opened.document).unwrap();
    assert_eq!(name, "mini.cuelight");
    let reopened = Opened::from_bytes(&name, &bytes).unwrap();
    assert_eq!(reopened.summary.size, [64, 33]);
    assert_eq!(reopened.files.len(), opened.files.len());

    let loose = std::fs::read(fixture("typed").join("show.json")).unwrap();
    let opened = Opened::from_bytes("typed.json", &loose).unwrap();
    let (name, bytes) = save::download(&opened.origin, &opened.files, &opened.document).unwrap();
    assert_eq!((name.as_str(), bytes), ("typed.json", loose));
}

#[test]
fn a_rename_saves_the_driver_with_the_show_and_opens_again() {
    use cuelight_editor_core::renames::{self, DRIVER, Kind};
    let dir = tempfile::tempdir().unwrap();
    let show = dir.path().join("mini");
    copy_dir(&fixture("mini"), &show);
    let mut opened = Opened::from_path(&show).unwrap();
    let driver = renames::driver_text(&opened.document, &opened.files).unwrap();
    let driver_value: Value = serde_json::from_str(&driver).unwrap();
    let plan = renames::plan(
        Kind::Variable,
        "lit",
        "on",
        &opened.document.value(),
        Some(&driver_value),
    )
    .unwrap();
    renames::apply(&plan, &mut opened.document, Some(&driver)).unwrap();
    save::save(&opened.origin, &mut opened.files, &mut opened.document).unwrap();
    let written = std::fs::read_to_string(show.join(DRIVER)).unwrap();
    assert_eq!(written, driver.replace("\"lit\"", "\"on\""), "as written");
    assert_eq!(opened.files[DRIVER], written.as_bytes(), "held as on disk");
    let again = Opened::from_path(&show).unwrap();
    assert!(
        again.summary.problems.is_empty(),
        "{:?}",
        again.summary.problems
    );
    assert_eq!(again.driver.unwrap().steps.len(), 5);
}
