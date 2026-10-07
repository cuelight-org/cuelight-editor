//! Assets managed in the editor: imported, used, renamed, replaced and
//! deleted, each undone with the edits it caused, and the show as the
//! engine has it after each.

// Test code throughout, so clippy lets it panic as tests do.
#![cfg(test)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cuelight_editor_core::assets::{Asset, Kind};
use cuelight_editor_core::document::{Document, Pointer};
use cuelight_editor_core::manage;
use cuelight_editor_core::opened::Opened;
use cuelight_editor_core::save::{self, Origin};
use serde_json::json;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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

/// Every file and folder under `dir`, files with their bytes.
fn tree(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.insert(name, None);
                walk(root, &path, out);
            } else {
                out.insert(name, Some(std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// The show as its edits leave it, opened again: what the engine has
/// on the next frame.
fn now(files: &BTreeMap<String, Vec<u8>>, document: &Document) -> Opened {
    let files = save::files_with(files, document);
    Opened::from_files("test", Origin::Bytes { name: "t".into() }, files).unwrap()
}

fn asset(opened: &Opened, name: &str) -> Asset {
    opened
        .library
        .iter()
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("no asset {name}"))
        .clone()
}

/// A short silent WAV of `samples` samples at 8 kHz.
fn silence(samples: u32) -> Vec<u8> {
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(samples * 2).to_le_bytes());
    wav.resize(wav.len() + samples as usize * 2, 0);
    wav
}

/// A 16 x 8 picture, unlike the mini show's 8 x 8 dot.
fn wide_png() -> Vec<u8> {
    std::fs::read(fixture("typed/assets/fonts/tiny.png")).unwrap()
}

fn editor_font(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../cuelight-editor/fonts")
            .join(name),
    )
    .unwrap()
}

#[test]
fn an_image_imported_used_renamed_and_deleted_undoes_to_the_folder_it_was() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture("mini"), dir.path());
    let before = tree(dir.path());
    let mut opened = Opened::from_path(dir.path()).unwrap();
    let (origin, files, document) = (&opened.origin, &mut opened.files, &mut opened.document);

    // Imported, and named apart from what is there.
    let imports = manage::import(document, files, &[("dot.png".into(), wide_png())]);
    assert_eq!(
        imports.imported,
        [(
            "dot-2".to_owned(),
            Kind::Image,
            "assets/dot-2.png".to_owned()
        )]
    );
    assert_eq!(
        now(files, document).engine.image("dot-2").unwrap().width,
        16
    );

    // Used by a layer.
    let at = Pointer::parse("/layers/2").unwrap();
    document
        .insert(
            &at,
            json!({ "name": "glow", "type": "image", "image": "dot-2" }),
        )
        .unwrap();
    let glow = asset(&now(files, document), "dot-2");
    assert_eq!(glow.uses.len(), 1);

    // Deleting it now is refused, saying what uses it.
    let refused = manage::delete(document, files, &glow).unwrap_err();
    assert!(refused.contains("glow"), "{refused}");

    // Renamed: the file and the layer, as one step.
    let steps = document.steps().len();
    assert_eq!(manage::rename(document, files, &glow, "halo").unwrap(), 1);
    assert_eq!(document.steps().len(), steps + 1);
    let renamed = now(files, document);
    assert_eq!(renamed.engine.image("halo").unwrap().width, 16);
    assert!(renamed.engine.image("dot-2").is_none());
    assert_eq!(document.value()["layers"][2]["image"], "halo");

    // A save writes the renamed file.
    save::save(origin, files, document).unwrap();
    assert!(dir.path().join("assets/halo.png").is_file());
    assert!(!dir.path().join("assets/dot-2.png").exists());

    // Unused again, then deleted.
    document.remove(&at).unwrap();
    let halo = asset(&now(files, document), "halo");
    manage::delete(document, files, &halo).unwrap();
    assert!(now(files, document).engine.image("halo").is_none());

    // Every step undone, and saved: the folder as it was.
    while document.undo() {}
    save::save(origin, files, document).unwrap();
    assert_eq!(tree(dir.path()), before);
}

#[test]
fn a_sound_imported_into_a_new_folder_takes_the_folder_with_it_when_undone() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture("mini"), dir.path());
    let before = tree(dir.path());
    let mut opened = Opened::from_path(dir.path()).unwrap();
    let (origin, files, document) = (&opened.origin, &mut opened.files, &mut opened.document);
    let imports = manage::import(
        document,
        files,
        &[
            ("/somewhere/ding.wav".into(), silence(800)),
            ("notes.txt".into(), b"hello".to_vec()),
        ],
    );
    assert_eq!(imports.imported.len(), 1);
    assert_eq!(imports.refused.len(), 1, "a text file is no asset");
    save::save(origin, files, document).unwrap();
    assert!(dir.path().join("assets/sounds/ding.wav").is_file());
    document.undo();
    save::save(origin, files, document).unwrap();
    assert_eq!(tree(dir.path()), before);
}

#[test]
fn each_kind_replaced_and_removed_is_what_the_engine_has_next() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture("mini"), dir.path());
    std::fs::create_dir_all(dir.path().join("assets/fonts")).unwrap();
    std::fs::create_dir_all(dir.path().join("assets/sounds")).unwrap();
    std::fs::write(
        dir.path().join("assets/fonts/mono.ttf"),
        editor_font("DMMono-Regular.ttf"),
    )
    .unwrap();
    std::fs::write(dir.path().join("assets/sounds/ding.wav"), silence(800)).unwrap();
    let square = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    std::fs::write(dir.path().join("assets/mark.svg"), square).unwrap();
    let mut opened = Opened::from_path(dir.path()).unwrap();
    let (files, document) = (&mut opened.files, &mut opened.document);

    // Replaced: an image, a vector, a font and a sound.
    let shown = now(files, document);
    manage::replace(
        document,
        files,
        &asset(&shown, "dot"),
        "re-export.png",
        wide_png(),
    )
    .unwrap();
    let wide = r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="10"><rect width="30" height="10"/></svg>"#;
    manage::replace(
        document,
        files,
        &asset(&shown, "mark"),
        "mark.svg",
        wide.into(),
    )
    .unwrap();
    let other = editor_font("AtkinsonHyperlegible-Regular.ttf");
    manage::replace(
        document,
        files,
        &asset(&shown, "mono"),
        "a.ttf",
        other.clone(),
    )
    .unwrap();
    manage::replace(
        document,
        files,
        &asset(&shown, "ding"),
        "b.wav",
        silence(1600),
    )
    .unwrap();
    let replaced = now(files, document);
    assert_eq!(replaced.engine.image("dot").unwrap().width, 16);
    assert_eq!(replaced.engine.vector("mark").unwrap().width, 30.0);
    let font = replaced.engine.outline_fonts().find(|(n, _)| *n == "mono");
    assert_eq!(font.map(|(_, bytes)| bytes.to_vec()), Some(other));
    assert!((replaced.engine.sound_duration("ding").unwrap() - 0.2).abs() < 1e-6);
    // A sound file is no picture, and stays what it was.
    assert!(manage::replace(document, files, &asset(&shown, "ding"), "x.png", wide_png()).is_err());

    // Removed: the dot's layer first, which uses it.
    document
        .remove(&Pointer::parse("/layers/1").unwrap())
        .unwrap();
    let shown = now(files, document);
    for name in ["dot", "mark", "mono", "ding"] {
        manage::delete(document, files, &asset(&shown, name)).unwrap();
    }
    let removed = now(files, document);
    assert!(removed.engine.image("dot").is_none());
    assert!(removed.engine.vector("mark").is_none());
    assert!(!removed.engine.has_font("mono"));
    assert!(removed.engine.sound_duration("ding").is_none());
    assert!(removed.library.is_empty());
}

#[test]
fn artwork_is_drawn_again_with_a_font_imported_after_it() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture("mini"), dir.path());
    let words = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="10"><text font-family="DM Mono" y="8">Hi</text></svg>"#;
    std::fs::write(dir.path().join("assets/words.svg"), words).unwrap();
    let mut opened = Opened::from_path(dir.path()).unwrap();
    let missing = |o: &Opened| o.summary.problems.iter().any(|p| p.contains("DM Mono"));
    assert!(missing(&opened));
    let (files, document) = (&mut opened.files, &mut opened.document);
    manage::import(
        document,
        files,
        &[(
            "DMMono-Regular.ttf".into(),
            editor_font("DMMono-Regular.ttf"),
        )],
    );
    assert!(!missing(&now(files, document)));
}

#[test]
fn a_font_renamed_renames_its_styles_and_a_style_keeps_it_from_deletion() {
    let mut opened = Opened::from_path(&fixture("typed")).unwrap();
    let (files, document) = (&mut opened.files, &mut opened.document);
    let tiny = asset(&now(files, document), "tiny");
    assert!(manage::delete(document, files, &tiny).is_err());
    assert_eq!(manage::rename(document, files, &tiny, "small").unwrap(), 2);
    let value = document.value();
    assert_eq!(value["fonts"]["plain"]["file"], "small");
    assert_eq!(value["fonts"]["loud"]["file"], "small");
    let renamed = now(files, document);
    assert!(renamed.engine.has_font("small"));
    // The pages stay where the font's description says they are.
    assert!(manage::now(document, files, "assets/fonts/tiny.png").is_some());
    document.undo();
    assert_eq!(document.value()["fonts"]["plain"]["file"], "tiny");
    assert!(now(files, document).engine.has_font("tiny"));
}

#[test]
fn a_rename_reaches_reel_cells_and_what_a_binding_switches_to() {
    let show = json!({
        "layers": [
            { "name": "pic", "type": "image", "image": "a",
              "bindings": [{ "property": "image", "variable": "v", "map": { "1": "a", "2": "b" }, "default": "a" }] },
            { "name": "g", "type": "group", "children": [
                { "name": "reel", "type": "digits", "display": { "reel": { "cells": { "vectors": ["b", "a"] } } } } ] },
            { "name": "a", "type": "shape" }
        ],
        "scenes": [{ "name": "s", "layers": [{ "name": "x", "type": "vector", "vector": "a" }] }]
    });
    let found: Vec<String> = manage::uses(&show, Kind::Vector, "a")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        found,
        [
            "/layers/0/image",
            "/layers/0/bindings/0/map/1",
            "/layers/0/bindings/0/default",
            "/layers/1/children/0/display/reel/cells/vectors/1",
            "/scenes/0/layers/0/vector",
        ]
    );
}
