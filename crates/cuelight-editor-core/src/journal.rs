//! The journal: a show's unsaved edits kept outside the show, so a
//! crash or a closed tab does not take them.
//!
//! An entry is the document's whole text as it stands after an edit,
//! with what it was edited from: a hash of `show.json` as opened. One
//! entry per show, keyed by where it came from, written over on every
//! edit and taken away once the show has no unsaved edits. Reopening
//! the show finds it and offers it back.
//!
//! On the desktop an entry is a file in the editor's data folder,
//! written whole or not at all; a browser keeps it in the page's
//! storage, which is the window's to reach.

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::save::Origin;

/// A show's unsaved edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The show it belongs to, as [`key`] names it.
    pub show: String,
    /// `show.json` as it was opened, hashed with [`base`].
    pub base: String,
    /// When it was written, in seconds since 1970.
    pub at: u64,
    /// The document as it stood.
    pub text: String,
}

impl Entry {
    pub fn to_json(&self) -> String {
        json!({
            "show": self.show,
            "base": self.base,
            "at": self.at,
            "text": self.text,
        })
        .to_string()
    }

    /// An entry read back; `None` for anything that is not one.
    pub fn from_json(text: &str) -> Option<Entry> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Entry {
            show: value.get("show")?.as_str()?.to_owned(),
            base: value.get("base")?.as_str()?.to_owned(),
            at: value.get("at")?.as_u64()?,
            text: value.get("text")?.as_str()?.to_owned(),
        })
    }
}

/// An entry found on opening a show, to offer back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub entry: Entry,
    /// The show changed since the edits were made from it: restoring
    /// them drops that change.
    pub changed: bool,
}

/// What an opened show's journal has to offer: nothing when there is no
/// entry, or it holds the text that was opened.
pub fn found(entry: Option<Entry>, opened: &[u8]) -> Option<Found> {
    let entry = entry?;
    if entry.text.as_bytes() == opened {
        return None;
    }
    let changed = entry.base != base(opened);
    Some(Found { entry, changed })
}

/// The name a show's entry goes by: its path on the desktop, its file
/// name for a show that came as bytes.
pub fn key(origin: &Origin) -> String {
    match origin {
        #[cfg(not(target_arch = "wasm32"))]
        Origin::Folder(path) | Origin::Pack(path) | Origin::Loose(path) => {
            let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            path.display().to_string()
        }
        Origin::Bytes { name } => name.clone(),
    }
}

/// A short fingerprint of `bytes` that stays the same from one run, and
/// one build, to the next: FNV-1a, in hex.
pub fn base(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// Now, in seconds since 1970.
pub fn now() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How long ago `at` was, in words: "3 minutes ago".
pub fn ago(at: u64, now: u64) -> String {
    let seconds = now.saturating_sub(at);
    let (count, unit) = match seconds {
        0..60 => return "moments ago".to_owned(),
        60..3600 => (seconds / 60, "minute"),
        3600..86_400 => (seconds / 3600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// The editor's own folder for journal entries: under the user's data
/// folder, `cuelight-editor/journal`. `None` without a home to put it in.
#[cfg(not(target_arch = "wasm32"))]
pub fn folder() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let data = if cfg!(windows) {
        PathBuf::from(var("LOCALAPPDATA")?)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(var("HOME")?).join("Library/Application Support")
    } else {
        match var("XDG_DATA_HOME") {
            Some(data) => PathBuf::from(data),
            None => PathBuf::from(var("HOME")?).join(".local/share"),
        }
    };
    Some(data.join("cuelight-editor").join("journal"))
}

/// The file a show's entry is kept in, named after a hash of its key.
#[cfg(not(target_arch = "wasm32"))]
fn file(folder: &Path, show: &str) -> PathBuf {
    folder.join(format!("{}.json", base(show.as_bytes())))
}

/// Write `entry` over the show's last one.
#[cfg(not(target_arch = "wasm32"))]
pub fn write(folder: &Path, entry: &Entry) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    crate::save::write(&file(folder, &entry.show), entry.to_json().as_bytes())
        .map_err(|e| e.to_string())
}

/// The show's entry, if one was left.
#[cfg(not(target_arch = "wasm32"))]
pub fn read(folder: &Path, show: &str) -> Option<Entry> {
    let text = std::fs::read_to_string(file(folder, show)).ok()?;
    Entry::from_json(&text).filter(|entry| entry.show == show)
}

/// Take the show's entry away; there being none is fine.
#[cfg(not(target_arch = "wasm32"))]
pub fn clear(folder: &Path, show: &str) -> Result<(), String> {
    match std::fs::remove_file(file(folder, show)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {e}", folder.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: &str = r#"{"format": 1, "name": "t", "size": [8, 8]}"#;

    fn entry(text: &str) -> Entry {
        Entry {
            show: "/shows/t".to_owned(),
            base: base(SHOW.as_bytes()),
            at: 1_000,
            text: text.to_owned(),
        }
    }

    #[test]
    fn an_entry_is_written_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("journal");
        let edited = SHOW.replace("\"t\"", "\"u\"");
        write(&folder, &entry(&edited)).unwrap();
        assert_eq!(read(&folder, "/shows/t"), Some(entry(&edited)));
        assert_eq!(read(&folder, "/shows/other"), None);

        let later = SHOW.replace("\"t\"", "\"v\"");
        write(&folder, &entry(&later)).unwrap();
        assert_eq!(
            read(&folder, "/shows/t").unwrap().text,
            later,
            "written over"
        );
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "one file");
    }

    #[test]
    fn an_entry_is_offered_when_it_holds_other_text() {
        let edited = SHOW.replace("\"t\"", "\"u\"");
        assert_eq!(
            found(Some(entry(&edited)), SHOW.as_bytes()),
            Some(Found {
                entry: entry(&edited),
                changed: false
            })
        );
        assert_eq!(found(Some(entry(SHOW)), SHOW.as_bytes()), None);
        assert_eq!(found(None, SHOW.as_bytes()), None);
    }

    #[test]
    fn a_show_that_changed_since_says_so() {
        let edited = SHOW.replace("\"t\"", "\"u\"");
        let on_disk = SHOW.replace("8, 8", "9, 9");
        let found = found(Some(entry(&edited)), on_disk.as_bytes()).unwrap();
        assert!(found.changed);
    }

    #[test]
    fn a_cleared_entry_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &entry("{}")).unwrap();
        clear(dir.path(), "/shows/t").unwrap();
        assert_eq!(read(dir.path(), "/shows/t"), None);
        clear(dir.path(), "/shows/t").unwrap();
    }

    #[test]
    fn age_reads_in_words() {
        assert_eq!(ago(100, 130), "moments ago");
        assert_eq!(ago(0, 60), "1 minute ago");
        assert_eq!(ago(0, 7_300), "2 hours ago");
        assert_eq!(ago(0, 3 * 86_400), "3 days ago");
        assert_eq!(ago(10, 0), "moments ago");
    }

    #[test]
    fn the_base_is_stable() {
        assert_eq!(base(b""), "cbf29ce484222325");
        assert_eq!(base(b"a"), "af63dc4c8601ec8c");
    }
}
