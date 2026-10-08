//! Managing the show's assets: files imported into it, and assets
//! replaced, renamed and deleted.
//!
//! Each is an edit of the document: a file added, changed or taken away
//! goes into its undo history together with the JSON edits it causes (a
//! rename renames every use), so one undo takes back both. The files
//! stay in memory until the next save writes them.
//!
//! A file goes where the loader looks for its kind: pictures and SVG
//! artwork in `assets/`, fonts in `assets/fonts/`, sounds in
//! `assets/sounds/` and clips in `assets/videos/`. An asset is named by
//! its file's stem, so a name is kept unique within its folder.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::assets::{Asset, Kind};
use crate::document::{Bytes, Document, EditError, Part, Pointer};

/// Where a file of this name goes in a show, and the kind of asset it
/// is there; `None` for a file the show has no use for.
pub fn place_for(file: &str) -> Option<(&'static str, Kind)> {
    let (_, extension) = file.rsplit_once('.')?;
    let extension = extension.to_ascii_lowercase();
    let extension = extension.as_str();
    Some(if extension == cuelight_loader::VECTOR_EXTENSION {
        ("assets", Kind::Vector)
    } else if cuelight_loader::IMAGE_EXTENSIONS.contains(&extension) {
        ("assets", Kind::Image)
    } else if matches!(extension, "fnt" | "ttf" | "otf") {
        ("assets/fonts", Kind::Font)
    } else if cuelight_loader::SOUND_EXTENSIONS.contains(&extension) {
        ("assets/sounds", Kind::Sound)
    } else if cuelight_loader::VIDEO_EXTENSIONS.contains(&extension) {
        ("assets/videos", Kind::Video)
    } else {
        return None;
    })
}

/// What an import made of the files it was given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imports {
    /// Each asset imported: its name, its kind and its path in the show.
    pub imported: Vec<(String, Kind, String)>,
    /// Each file left out, with why.
    pub refused: Vec<String>,
}

/// Import files, each by its file name and bytes, as one step. A bitmap
/// font's pages go with it into `assets/fonts/` when they are among the
/// files, under the names the font gives them.
pub fn import(
    document: &mut Document,
    files: &BTreeMap<String, Vec<u8>>,
    picked: &[(String, Vec<u8>)],
) -> Imports {
    let mut out = Imports::default();
    let pages: BTreeSet<String> = picked
        .iter()
        .filter(|(name, _)| extension(name) == "fnt")
        .flat_map(|(_, bytes)| font_pages(bytes))
        .collect();
    document.begin_step();
    for (name, bytes) in picked {
        let name = file_name(name);
        if pages.contains(name) {
            let path = format!("assets/fonts/{name}");
            match now(document, files, &path) {
                Some(there) if there == bytes.as_slice() => {}
                Some(_) => out
                    .refused
                    .push(format!("{name}: another font page is already called that")),
                None => put(document, files, &path, Some(bytes.clone())),
            }
            continue;
        }
        let Some((folder, kind)) = place_for(name) else {
            out.refused
                .push(format!("{name}: not a kind of file a show can use"));
            continue;
        };
        let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
        let stem = free_stem(document, files, folder, stem);
        let path = format!("{folder}/{stem}.{extension}");
        put(document, files, &path, Some(bytes.clone()));
        out.imported.push((stem, kind, path));
    }
    document.end_step();
    out
}

/// Take an asset's file out of the show. Refused while anything uses
/// it, with what does: the layers, and for a font the styles that name
/// it. A bitmap font's pages go with it.
pub fn delete(
    document: &mut Document,
    files: &BTreeMap<String, Vec<u8>>,
    asset: &Asset,
) -> Result<(), String> {
    let mut users: Vec<String> = asset.uses.iter().map(|u| u.place.clone()).collect();
    // What the document names it by, should the uses be behind it.
    if users.is_empty() {
        users.extend(
            uses(&document.value(), asset.kind, &asset.name)
                .iter()
                .map(ToString::to_string),
        );
    }
    if asset.kind == Kind::Font {
        users.extend(styles_of(&document.value(), &asset.name).map(|s| format!("font style {s}")));
    }
    users.dedup();
    if !users.is_empty() {
        return Err(format!(
            "{} is used by {}; take those uses out first",
            asset.name,
            users.join(", ")
        ));
    }
    let path = asset
        .file
        .clone()
        .ok_or("this asset has no file in the show")?;
    let pages = match now(document, files, &path) {
        Some(bytes) if extension(&path) == "fnt" => font_pages(bytes),
        _ => Vec::new(),
    };
    document.begin_step();
    put(document, files, &path, None);
    for page in pages {
        let page = format!("assets/fonts/{page}");
        if now(document, files, &page).is_some() && !paged_by_another(document, files, &page) {
            put(document, files, &page, None);
        }
    }
    document.end_step();
    Ok(())
}

/// Give an asset a new name: its file is renamed, and every place the
/// show names it, as one step. Returns how many places were renamed.
pub fn rename(
    document: &mut Document,
    files: &BTreeMap<String, Vec<u8>>,
    asset: &Asset,
    to: &str,
) -> Result<usize, String> {
    let to = to.trim();
    if to == asset.name {
        return Ok(0);
    }
    if to.is_empty() || to.starts_with('.') || to.contains(['/', '\\']) {
        return Err(format!("{to:?} cannot name a file"));
    }
    let path = asset
        .file
        .clone()
        .ok_or("this asset has no file in the show")?;
    let (folder, file) = path.rsplit_once('/').ok_or("this asset has no folder")?;
    let extension = file.rsplit_once('.').map_or("", |(_, e)| e);
    if stem_taken(document, files, folder, to) {
        return Err(format!("{folder} already has a file called {to}"));
    }
    let bytes = now(document, files, &path)
        .ok_or("this asset's file is gone")?
        .to_vec();
    let places = uses(&document.value(), asset.kind, &asset.name);
    let steps = document.steps().len();
    document.begin_step();
    let done = (|| -> Result<(), EditError> {
        put(
            document,
            files,
            &format!("{folder}/{to}.{extension}"),
            Some(bytes),
        );
        put(document, files, &path, None);
        for at in &places {
            document.set(at, Value::String(to.to_owned()))?;
        }
        Ok(())
    })();
    document.end_step();
    if let Err(error) = done {
        if document.steps().len() > steps {
            document.undo();
        }
        return Err(error.to_string());
    }
    Ok(places.len())
}

/// Give an asset's file new bytes, from a file of this name: the same
/// kind of file, which keeps the asset's name. A file of another format
/// for the same folder (a PNG for an SVG) takes the old one's place.
pub fn replace(
    document: &mut Document,
    files: &BTreeMap<String, Vec<u8>>,
    asset: &Asset,
    name: &str,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let path = asset
        .file
        .clone()
        .ok_or("this asset has no file in the show")?;
    let (folder, _) = path.rsplit_once('/').ok_or("this asset has no folder")?;
    let name = file_name(name);
    match place_for(name) {
        Some((place, _)) if place == folder => {}
        _ => {
            return Err(format!(
                "{name} is not a file for {folder}, where {} is",
                asset.name
            ));
        }
    }
    let new = format!("{folder}/{}.{}", asset.name, extension_as_written(name));
    document.begin_step();
    if new != path {
        put(document, files, &path, None);
    }
    put(document, files, &new, Some(bytes));
    document.end_step();
    Ok(())
}

/// The places the show names an asset of `kind` called `name`, as
/// pointers to the text that names it: an image layer's artwork, a reel
/// cell, an audio layer's sound or a video layer's clip, what a binding
/// can switch them to, and a font style's file.
pub fn uses(show: &Value, kind: Kind, name: &str) -> Vec<Pointer> {
    let mut out = Vec::new();
    if kind == Kind::Font {
        for style in styles_of(show, name) {
            out.push(pointer(&["fonts", &style, "file"]));
        }
        return out;
    }
    let root = Pointer::default();
    walk(
        show.get("layers"),
        &root.then(key("layers")),
        kind,
        name,
        &mut out,
    );
    if let Some(scenes) = show.get("scenes").and_then(Value::as_array) {
        for (i, scene) in scenes.iter().enumerate() {
            let at = root.then(key("scenes")).then(Part::Index(i));
            walk(
                scene.get("layers"),
                &at.then(key("layers")),
                kind,
                name,
                &mut out,
            );
        }
    }
    out
}

/// The layers in `list`, at `at`, and their children and parts.
fn walk(list: Option<&Value>, at: &Pointer, kind: Kind, name: &str, out: &mut Vec<Pointer>) {
    let Some(layers) = list.and_then(Value::as_array) else {
        return;
    };
    for (i, layer) in layers.iter().enumerate() {
        let here = at.then(Part::Index(i));
        let typed = layer.get("type").and_then(Value::as_str).unwrap_or("");
        let named = |field: &str, out: &mut Vec<Pointer>| {
            names_in(layer.get(field), &here.then(key(field)), name, out);
        };
        let property = match kind {
            Kind::Image | Kind::Vector => {
                if matches!(typed, "image" | "vector") {
                    named("image", out);
                    named("vector", out);
                }
                if let Some(cells) = layer.pointer("/display/reel/cells") {
                    let cells_at = here
                        .then(key("display"))
                        .then(key("reel"))
                        .then(key("cells"));
                    for field in ["vectors", "images"] {
                        names_in(cells.get(field), &cells_at.then(key(field)), name, out);
                    }
                }
                "image"
            }
            Kind::Sound => {
                if typed == "audio" {
                    named("sound", out);
                }
                "sound"
            }
            Kind::Video => {
                if typed == "video" {
                    named("video", out);
                }
                "video"
            }
            Kind::Font => "",
        };
        if let Some(bindings) = layer.get("bindings").and_then(Value::as_array) {
            for (b, binding) in bindings.iter().enumerate() {
                if binding.get("property").and_then(Value::as_str) != Some(property) {
                    continue;
                }
                let at = here.then(key("bindings")).then(Part::Index(b));
                if let Some(map) = binding.get("map").and_then(Value::as_object) {
                    for (k, v) in map {
                        if v.as_str() == Some(name) {
                            out.push(at.then(key("map")).then(key(k)));
                        }
                    }
                }
                if binding.get("default").and_then(Value::as_str) == Some(name) {
                    out.push(at.then(key("default")));
                }
            }
        }
        for nested in ["children", "parts"] {
            walk(layer.get(nested), &here.then(key(nested)), kind, name, out);
        }
    }
}

/// `value` at `at` where it is the text `name`, or each item of a list
/// that is.
fn names_in(value: Option<&Value>, at: &Pointer, name: &str, out: &mut Vec<Pointer>) {
    match value {
        Some(Value::String(text)) if text == name => out.push(at.clone()),
        Some(Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                if item.as_str() == Some(name) {
                    out.push(at.then(Part::Index(i)));
                }
            }
        }
        _ => {}
    }
}

/// The font styles whose file is the font `name`.
fn styles_of<'a>(show: &'a Value, name: &'a str) -> impl Iterator<Item = String> + 'a {
    show.get("fonts")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(move |(_, style)| style.get("file").and_then(Value::as_str) == Some(name))
        .map(|(style, _)| style.clone())
}

fn key(name: &str) -> Part {
    Part::Key(name.to_owned())
}

fn pointer(keys: &[&str]) -> Pointer {
    Pointer(keys.iter().map(|k| key(k)).collect())
}

/// A file's bytes as the show has them now: as the edits left them, or
/// as it shipped.
pub fn now<'a>(
    document: &'a Document,
    files: &'a BTreeMap<String, Vec<u8>>,
    path: &str,
) -> Option<&'a [u8]> {
    match document.files().get(path) {
        Some(edited) => edited.as_deref(),
        None => files.get(path).map(Vec::as_slice),
    }
}

/// Put a file's new bytes, or take it away, as an edit.
fn put(
    document: &mut Document,
    files: &BTreeMap<String, Vec<u8>>,
    path: &str,
    bytes: Option<Vec<u8>>,
) {
    let shipped = files.get(path).map(Vec::as_slice);
    document.put_file(path, shipped, bytes.map(Bytes::from));
}

/// The paths of the show's files now.
fn paths_now(document: &Document, files: &BTreeMap<String, Vec<u8>>) -> BTreeSet<String> {
    let mut paths: BTreeSet<String> = files.keys().cloned().collect();
    for (path, bytes) in document.files() {
        match bytes {
            Some(_) => paths.insert(path.clone()),
            None => paths.remove(path),
        };
    }
    paths
}

/// Whether a file directly in `folder` has the stem `stem`.
fn stem_taken(
    document: &Document,
    files: &BTreeMap<String, Vec<u8>>,
    folder: &str,
    stem: &str,
) -> bool {
    let prefix = format!("{folder}/{stem}.");
    paths_now(document, files).iter().any(|path| {
        path.strip_prefix(&prefix)
            .is_some_and(|rest| !rest.contains('/'))
    })
}

/// `stem`, or `stem-2`, `stem-3`, ... : the first no file in `folder`
/// has.
fn free_stem(
    document: &Document,
    files: &BTreeMap<String, Vec<u8>>,
    folder: &str,
    stem: &str,
) -> String {
    if !stem_taken(document, files, folder, stem) {
        return stem.to_owned();
    }
    (2..)
        .map(|n| format!("{stem}-{n}"))
        .find(|candidate| !stem_taken(document, files, folder, candidate))
        .unwrap_or_else(|| stem.to_owned())
}

/// Whether another bitmap font in `assets/fonts/` has `page` as a page.
fn paged_by_another(document: &Document, files: &BTreeMap<String, Vec<u8>>, page: &str) -> bool {
    let page = page.rsplit('/').next().unwrap_or(page);
    paths_now(document, files)
        .iter()
        .filter(|p| p.starts_with("assets/fonts/") && extension(p) == "fnt")
        .filter_map(|p| now(document, files, p))
        .any(|fnt| font_pages(fnt).iter().any(|named| named == page))
}

/// The page images a BMFont text description names.
pub fn font_pages(fnt: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(fnt)
        .lines()
        .filter(|line| line.trim_start().starts_with("page "))
        .filter_map(|line| {
            let (_, rest) = line.split_once("file=\"")?;
            let (file, _) = rest.split_once('"')?;
            Some(file.to_owned())
        })
        .collect()
}

/// A path's last part.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A file's extension in lower case.
fn extension(path: &str) -> String {
    file_name(path)
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// A file's extension as its name writes it.
fn extension_as_written(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(_, e)| e)
}
