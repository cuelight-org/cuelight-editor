//! Telling a change made to a show on disk from outside the editor (a
//! text editor, a script that writes the show) from the editor's own
//! saves.
//!
//! No timing is involved: what is on disk now is read and compared with
//! the files the editor last opened or saved. The same files are the
//! editor's own write, or a touch; anything else is a change from
//! outside, and says which files it changed.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use crate::opened::{self, OpenError};
use crate::save::Origin;

/// A show's files as they are on disk now, and what differs from the
/// files the editor holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub files: BTreeMap<String, Vec<u8>>,
    /// The show document is not what the editor holds.
    pub document: bool,
    /// The other files added, changed or removed: the driver, assets.
    pub others: Vec<String>,
}

/// Where the show lives on disk, to watch and to open again; nothing
/// for a show that came as bytes.
pub fn path(origin: &Origin) -> Option<&Path> {
    match origin {
        Origin::Folder(path) | Origin::Pack(path) | Origin::Loose(path) => Some(path),
        Origin::Bytes { .. } => None,
    }
}

/// What to watch for a show: a folder with everything under it, or the
/// folder holding a pack or loose document (a file written by moving
/// another over it is a new file, which a watch on the old one misses).
/// The flag says whether to watch under it.
pub fn watched(origin: &Origin) -> Option<(&Path, bool)> {
    match origin {
        Origin::Folder(dir) => Some((dir, true)),
        Origin::Pack(file) | Origin::Loose(file) => Some((file.parent()?, false)),
        Origin::Bytes { .. } => None,
    }
}

/// Whether a path the watch reported is part of the show: in its
/// folder and not hidden (the editor's save writes a hidden file first),
/// or the pack or loose document itself.
pub fn concerns(origin: &Origin, changed: &Path) -> bool {
    match origin {
        Origin::Folder(dir) => changed.strip_prefix(dir).is_ok_and(|inside| {
            !inside.components().any(|c| match c {
                Component::Normal(name) => name.to_string_lossy().starts_with('.'),
                _ => false,
            })
        }),
        Origin::Pack(file) | Origin::Loose(file) => changed == file,
        Origin::Bytes { .. } => false,
    }
}

/// Read the show's files from disk and compare them with `held`, what
/// the editor last opened or saved: `None` when they are the same.
pub fn change(
    origin: &Origin,
    held: &BTreeMap<String, Vec<u8>>,
) -> Result<Option<Change>, OpenError> {
    let Some(path) = path(origin) else {
        return Ok(None);
    };
    let files = opened::files_on_disk(path)?;
    if &files == held {
        return Ok(None);
    }
    let document = files.get("show.json") != held.get("show.json");
    let others = files
        .keys()
        .chain(held.keys())
        .filter(|name| name.as_str() != "show.json" && files.get(*name) != held.get(*name))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .cloned()
        .collect();
    Ok(Some(Change {
        files,
        document,
        others,
    }))
}
