//! Saving a show back where it came from, in the form it came in.
//!
//! The document is written as [`Document::text`] has it: untouched nodes
//! exactly as read, so a show saved without edits is the same file, and
//! an edit changes the lines it touched. Every other file the show
//! shipped goes back as it was opened.
//!
//! A folder gets its `show.json` rewritten, a pack is packed again, a
//! loose document is written in place. A show that came as bytes (a
//! browser's pick or drop, a fetched URL) has no place to go back to:
//! it is handed back as a download of the same name.

use std::collections::BTreeMap;
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use crate::document::Document;

/// Where an open show came from, which is where a save writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A show folder: `show.json` beside its assets.
    #[cfg(not(target_arch = "wasm32"))]
    Folder(PathBuf),
    /// A packed show, `.cuelight`.
    #[cfg(not(target_arch = "wasm32"))]
    Pack(PathBuf),
    /// A show document on its own.
    #[cfg(not(target_arch = "wasm32"))]
    Loose(PathBuf),
    /// Bytes handed over by name: a pack, or a loose document when the
    /// name ends in `.json`.
    Bytes { name: String },
}

#[derive(Debug)]
pub enum SaveError {
    /// The document asks for a format this editor does not know, and
    /// would be written in a form it cannot vouch for.
    NewerFormat { found: u32, known: u32 },
    /// The show came as bytes and has nowhere to be written back to.
    NoPlace,
    /// The pack could not be made, or a file not written.
    Write(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::NewerFormat { found, known } => write!(
                f,
                "this show is written for format {found}; this editor knows format {known}"
            ),
            SaveError::NoPlace => f.write_str("this show came without a place to save it to"),
            SaveError::Write(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SaveError {}

/// The show's files with the document as it stands now.
pub fn files_with(
    files: &BTreeMap<String, Vec<u8>>,
    document: &Document,
) -> BTreeMap<String, Vec<u8>> {
    let mut files = files.clone();
    files.insert("show.json".to_owned(), document.text().into_bytes());
    files
}

/// Write the show back where it came from. On success `files` holds
/// what is on disk now and the document is marked saved.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(
    origin: &Origin,
    files: &mut BTreeMap<String, Vec<u8>>,
    document: &mut Document,
) -> Result<(), SaveError> {
    check_format(document)?;
    let text = document.text().into_bytes();
    match origin {
        Origin::Folder(dir) => write(&dir.join("show.json"), &text)?,
        Origin::Pack(path) => {
            let bytes = cuelight_loader::pack_bytes(&files_with(files, document))
                .map_err(|e| SaveError::Write(e.to_string()))?;
            write(path, &bytes)?;
        }
        Origin::Loose(path) => write(path, &text)?,
        Origin::Bytes { .. } => return Err(SaveError::NoPlace),
    }
    files.insert("show.json".to_owned(), text);
    document.mark_saved();
    Ok(())
}

/// The file to hand back for a show that came as bytes, or to save a
/// copy of any show as: its name and bytes, a pack unless the show was
/// a loose document.
pub fn download(
    origin: &Origin,
    files: &BTreeMap<String, Vec<u8>>,
    document: &Document,
) -> Result<(String, Vec<u8>), SaveError> {
    check_format(document)?;
    let name = match origin {
        #[cfg(not(target_arch = "wasm32"))]
        Origin::Folder(path) | Origin::Pack(path) | Origin::Loose(path) => path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "show".to_owned()),
        Origin::Bytes { name } => name.clone(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    let loose = matches!(origin, Origin::Loose(_));
    #[cfg(target_arch = "wasm32")]
    let loose = false;
    if loose || name.ends_with(".json") {
        return Ok((name, document.text().into_bytes()));
    }
    let bytes = cuelight_loader::pack_bytes(&files_with(files, document))
        .map_err(|e| SaveError::Write(e.to_string()))?;
    let name = match name.strip_suffix(&format!(".{}", cuelight_loader::PACK_EXTENSION)) {
        Some(_) => name,
        None => format!("{name}.{}", cuelight_loader::PACK_EXTENSION),
    };
    Ok((name, bytes))
}

/// Refuse to write a document that asks for a newer format than this
/// editor knows.
fn check_format(document: &Document) -> Result<(), SaveError> {
    let found = document
        .value()
        .get("format")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let known = cuelight_core::FORMAT;
    if found > u64::from(known) {
        return Err(SaveError::NewerFormat {
            found: u32::try_from(found).unwrap_or(u32::MAX),
            known,
        });
    }
    Ok(())
}

/// Write a file whole or not at all: into a file beside it first, then
/// moved over it, so a crash mid-write leaves the old file standing.
#[cfg(not(target_arch = "wasm32"))]
fn write(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    let error = |e: std::io::Error| SaveError::Write(format!("{}: {e}", path.display()));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = path.with_file_name(format!(".{name}.saving"));
    std::fs::write(&partial, bytes).map_err(error)?;
    std::fs::rename(&partial, path).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        error(e)
    })
}
