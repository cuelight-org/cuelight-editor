//! A show as the editor holds it once opened: the engine it loaded into
//! and a summary of what came with it.
//!
//! A show is opened leniently: what cannot be understood is dropped and
//! reported, so an unfinished show still shows what it has. Only a show
//! with no document at all fails to open.

use std::collections::BTreeMap;
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;

use cuelight::Engine;
pub use cuelight_audio::Sound;
use cuelight_core::{Layer, Show};
use cuelight_loader::Options;
pub use cuelight_loader::SoundFile;

use crate::assets::{self, Asset, Names};
use crate::document::Document;
use crate::save::Origin;

/// What was opened, where from, and what it contained.
pub struct Opened {
    /// Where it came from: a path on the desktop, a file name in the
    /// browser.
    pub source: String,
    /// Where a save writes it back to.
    pub origin: Origin,
    pub engine: Engine,
    pub summary: Summary,
    /// The driver that came with the show, to play it by.
    pub driver: Option<cuelight_loader::Driver>,
    /// The show's sound files as shipped, for a host whose browser decodes
    /// them itself.
    pub sound_files: Vec<SoundFile>,
    /// The show's sounds, decoded, for a host with a sound device. Their
    /// lengths are already registered with the engine.
    pub sounds: Vec<(String, Arc<Sound>)>,
    /// Every file the show shipped, by its path within the show: the
    /// document, the driver and the assets. Kept from open to save.
    pub files: BTreeMap<String, Vec<u8>>,
    /// The assets the engine registered, with their facts and uses.
    pub library: Vec<Asset>,
    /// The show document as the editor edits it, text and all.
    pub document: Document,
}

/// The facts about a show worth showing before there is a stage.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    pub name: String,
    pub format: u32,
    pub size: [u32; 2],
    /// Layers in the show and in its scenes, groups' children and artwork
    /// layers' parts included.
    pub layers: usize,
    pub scenes: usize,
    pub variables: usize,
    pub values: usize,
    pub font_styles: usize,
    /// Keys the show listens to, and layers that can be pressed.
    pub keys: usize,
    pub pressable: usize,
    pub images: usize,
    pub vectors: usize,
    pub fonts: usize,
    pub sounds: usize,
    pub videos: usize,
    /// The driver's steps, and whether it loops; `None` without a driver.
    pub driver: Option<(usize, bool)>,
    /// Everything the load had to say: what it dropped and where, the
    /// engine's warnings, assets the build could not read, font families
    /// the artwork asked for.
    pub problems: Vec<String>,
}

#[derive(Debug)]
pub enum OpenError {
    /// The show asks for a format this editor does not know.
    NewerFormat {
        found: u32,
        known: u32,
    },
    Load(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NewerFormat { found, known } => write!(
                f,
                "this show is written for format {found}; this editor knows format {known}"
            ),
            OpenError::Load(message) => f.write_str(message),
        }
    }
}

impl Opened {
    /// Open a show folder, a packed show or a loose show file on disk.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_path(path: &Path) -> Result<Self, OpenError> {
        let document = document_on_disk(path)?;
        check_format(&document)?;
        let files = files_on_disk(path)?;
        let mut engine = Engine::new();
        let loaded = cuelight_loader::load_with(&mut engine, path, &Options::lenient())
            .map_err(|e| OpenError::Load(e.to_string()))?;
        let mut summary = summarize(&engine);
        summary.images = loaded.images.len();
        summary.vectors = loaded.vectors.len();
        summary.fonts = loaded.fonts.len();
        summary.videos = loaded.videos.len();
        summary.driver = loaded.driver.as_ref().map(|d| (d.steps.len(), d.looping));
        let sounds = decode_sounds(&mut engine, &loaded.sounds, &mut summary);
        let names = Names {
            images: loaded.images.clone(),
            vectors: loaded.vectors.clone(),
            fonts: loaded.fonts.clone(),
            sounds: loaded.sounds.iter().map(|s| s.name.clone()).collect(),
            videos: loaded
                .videos
                .iter()
                .filter_map(|p| Some(p.file_stem()?.to_string_lossy().into_owned()))
                .collect(),
        };
        let library = assets::library(&engine, &names, &files);
        summary
            .problems
            .extend(loaded.findings.iter().map(ToString::to_string));
        summary
            .problems
            .extend(loaded.skipped.iter().map(|p| format!("skipped {p}")));
        summary.problems.extend(
            loaded
                .missing_fonts
                .iter()
                .map(|f| format!("artwork asks for font {f:?}, which the show does not ship")),
        );
        let document = document_of(&files)?;
        let origin = if path.is_dir() {
            Origin::Folder(path.to_owned())
        } else if path
            .extension()
            .is_some_and(|e| e == cuelight_loader::PACK_EXTENSION)
        {
            Origin::Pack(path.to_owned())
        } else {
            Origin::Loose(path.to_owned())
        };
        Ok(Self {
            source: path.display().to_string(),
            origin,
            engine,
            summary,
            driver: loaded.driver,
            sound_files: loaded.sounds,
            sounds,
            files,
            library,
            document,
        })
    }

    /// Open a show from bytes: a packed show, or a loose show file by
    /// its name. What a browser hands over, and what a test packs.
    pub fn from_bytes(name: &str, bytes: &[u8]) -> Result<Self, OpenError> {
        let files: BTreeMap<String, Vec<u8>> = if name.ends_with(".json") {
            BTreeMap::from([("show.json".to_owned(), bytes.to_vec())])
        } else {
            cuelight_loader::unpack(bytes).map_err(|e| OpenError::Load(e.to_string()))?
        };
        if !files.contains_key("show.json") {
            return Err(OpenError::Load(format!("{name} holds no show.json")));
        }
        let origin = Origin::Bytes {
            name: name.to_owned(),
        };
        Self::from_files(name, origin, files)
    }

    /// Open a show from its files held in memory, by their paths within
    /// the show: what the bytes of a pack hold, or a show's files as its
    /// edits left them, to register its assets again after one changed.
    pub fn from_files(
        source: &str,
        origin: Origin,
        files: BTreeMap<String, Vec<u8>>,
    ) -> Result<Self, OpenError> {
        let document = files
            .get("show.json")
            .ok_or_else(|| OpenError::Load(format!("{source} holds no show.json")))?;
        check_format(&String::from_utf8_lossy(document))?;
        let mut engine = Engine::new();
        let loaded =
            cuelight_loader::load_from_memory_with(&mut engine, &files, &Options::lenient())
                .map_err(|e| OpenError::Load(e.to_string()))?;
        let mut summary = summarize(&engine);
        let videos = video_names(&files);
        summary.images = loaded.images.len();
        summary.vectors = loaded.vectors.len();
        summary.fonts = loaded.fonts.len();
        summary.videos = videos.len();
        summary.driver = loaded.driver.as_ref().map(|d| (d.steps.len(), d.looping));
        let sounds = decode_sounds(&mut engine, &loaded.sounds, &mut summary);
        let names = Names {
            images: loaded.images.clone(),
            vectors: loaded.vectors.clone(),
            fonts: loaded.fonts.clone(),
            sounds: loaded.sounds.iter().map(|s| s.name.clone()).collect(),
            videos,
        };
        let library = assets::library(&engine, &names, &files);
        summary
            .problems
            .extend(loaded.findings.iter().map(ToString::to_string));
        summary
            .problems
            .extend(loaded.skipped.iter().map(|p| format!("skipped {p}")));
        summary.problems.extend(
            loaded
                .missing_fonts
                .iter()
                .map(|f| format!("artwork asks for font {f:?}, which the show does not ship")),
        );
        let document = document_of(&files)?;
        Ok(Self {
            source: source.to_owned(),
            origin,
            engine,
            summary,
            driver: loaded.driver,
            sound_files: loaded.sounds,
            sounds,
            files,
            library,
            document,
        })
    }
}

/// The clips among the files, by name: what is directly in
/// `assets/videos/` with a video's extension.
fn video_names(files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    files
        .keys()
        .filter_map(|path| {
            let file = path.strip_prefix("assets/videos/")?;
            let (stem, extension) = file.rsplit_once('.')?;
            let video = cuelight_loader::VIDEO_EXTENSIONS
                .contains(&extension.to_ascii_lowercase().as_str());
            (video && !file.contains('/')).then(|| stem.to_owned())
        })
        .collect()
}

/// The show document among the files, as the editor edits it.
fn document_of(files: &BTreeMap<String, Vec<u8>>) -> Result<Document, OpenError> {
    let bytes = files
        .get("show.json")
        .ok_or_else(|| OpenError::Load("no show.json".to_owned()))?;
    let text = String::from_utf8_lossy(bytes);
    Document::parse(&text).map_err(|e| OpenError::Load(format!("show.json: {e}")))
}

/// Decode the show's sounds and tell the engine how long each is, so a
/// sound's `on_end` fires when it should. A sound that does not decode
/// is a problem, not a failure to open.
fn decode_sounds(
    engine: &mut Engine,
    files: &[SoundFile],
    summary: &mut Summary,
) -> Vec<(String, Arc<Sound>)> {
    let mut sounds = Vec::new();
    for file in files {
        match Sound::decode(&file.extension, &file.bytes) {
            Ok(sound) => match engine.set_sound(&file.name, sound.duration()) {
                Ok(()) => {
                    summary.sounds += 1;
                    sounds.push((file.name.clone(), Arc::new(sound)));
                }
                Err(error) => summary
                    .problems
                    .push(format!("sound {}: {error}", file.name)),
            },
            Err(error) => summary
                .problems
                .push(format!("sound {} does not decode: {error}", file.name)),
        }
    }
    sounds
}

/// Every file a show on disk ships, by its path within the show: what a
/// pack holds, what a folder's manifest lists, or the loose document alone.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn files_on_disk(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, OpenError> {
    let load = |e: cuelight_loader::LoadError| OpenError::Load(e.to_string());
    if path.is_dir() {
        let manifest = cuelight_loader::Manifest::for_dir(path).map_err(load)?;
        let mut files = BTreeMap::new();
        for name in manifest.files {
            let bytes = std::fs::read(path.join(&name))
                .map_err(|e| OpenError::Load(format!("{name}: {e}")))?;
            files.insert(name, bytes);
        }
        return Ok(files);
    }
    if path
        .extension()
        .is_some_and(|e| e == cuelight_loader::PACK_EXTENSION)
    {
        return cuelight_loader::read_pack(path).map_err(load);
    }
    let bytes =
        std::fs::read(path).map_err(|e| OpenError::Load(format!("{}: {e}", path.display())))?;
    Ok(BTreeMap::from([("show.json".to_owned(), bytes)]))
}

/// The show document's text for a path of any of the three forms, read
/// only to check its format before the loader takes it.
#[cfg(not(target_arch = "wasm32"))]
fn document_on_disk(path: &Path) -> Result<String, OpenError> {
    let read = |p: &Path| {
        std::fs::read_to_string(p).map_err(|e| OpenError::Load(format!("{}: {e}", p.display())))
    };
    if path.is_dir() {
        return read(&path.join("show.json"));
    }
    if path
        .extension()
        .is_some_and(|e| e == cuelight_loader::PACK_EXTENSION)
    {
        let files = cuelight_loader::read_pack(path).map_err(|e| OpenError::Load(e.to_string()))?;
        return files
            .get("show.json")
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .ok_or_else(|| OpenError::Load(format!("{} holds no show.json", path.display())));
    }
    read(path)
}

/// Refuse a show written for a newer format before the engine does,
/// with a message that says which version it wants.
fn check_format(document: &str) -> Result<(), OpenError> {
    let found = serde_json::from_str::<serde_json::Value>(document)
        .ok()
        .and_then(|v| v.get("format")?.as_u64())
        .unwrap_or(1) as u32;
    if found > cuelight_core::FORMAT {
        return Err(OpenError::NewerFormat {
            found,
            known: cuelight_core::FORMAT,
        });
    }
    Ok(())
}

/// The summary after the document was loaded again on its own: what the
/// engine says now, with the assets and driver as they were counted.
pub fn resummarize(engine: &Engine, before: &Summary, findings: &[String]) -> Summary {
    let mut summary = summarize(engine);
    summary.images = before.images;
    summary.vectors = before.vectors;
    summary.fonts = before.fonts;
    summary.sounds = before.sounds;
    summary.videos = before.videos;
    summary.driver = before.driver;
    summary.problems.extend(findings.iter().cloned());
    summary
}

fn summarize(engine: &Engine) -> Summary {
    let Some(show) = engine.show() else {
        return Summary::default();
    };
    let mut summary = Summary {
        name: show.name.clone(),
        format: show.format,
        size: show.size,
        scenes: show.scenes.len(),
        variables: show.variables.len(),
        values: show.values.len(),
        font_styles: show.fonts.len(),
        keys: show.input.keys.len(),
        problems: engine.load_warnings().to_vec(),
        ..Summary::default()
    };
    count(&show.layers, &mut summary);
    for scene in &show.scenes {
        count(&scene.layers, &mut summary);
    }
    summary
}

fn count(layers: &[Layer], summary: &mut Summary) {
    for layer in layers {
        summary.layers += 1;
        if layer.press.is_some() {
            summary.pressable += 1;
        }
        count(layer.children(), summary);
    }
}

/// The summary as lines for a plain read-out, or a log.
pub fn lines(summary: &Summary) -> Vec<(String, String)> {
    let Summary {
        name,
        format,
        size,
        layers,
        scenes,
        variables,
        values,
        font_styles,
        keys,
        pressable,
        images,
        vectors,
        fonts,
        sounds,
        videos,
        driver,
        ..
    } = summary;
    let mut out = vec![
        ("show".to_owned(), format!("{name} (format {format})")),
        ("canvas".to_owned(), format!("{} x {}", size[0], size[1])),
        (
            "layers".to_owned(),
            format!("{layers}, in {scenes} scene(s)"),
        ),
        (
            "variables".to_owned(),
            format!("{variables}, and {values} value(s) of the show's own"),
        ),
        ("font styles".to_owned(), font_styles.to_string()),
        (
            "input".to_owned(),
            format!("{keys} key(s), {pressable} pressable layer(s)"),
        ),
        (
            "assets".to_owned(),
            format!(
                "{images} image(s), {vectors} vector(s), {fonts} font(s), {sounds} sound(s), {videos} video(s)"
            ),
        ),
    ];
    out.push((
        "driver".to_owned(),
        match driver {
            Some((steps, true)) => format!("{steps} step(s), looping"),
            Some((steps, false)) => format!("{steps} step(s)"),
            None => "none".to_owned(),
        },
    ));
    out
}

// Keep `Show` in the public surface of this module for callers that
// want more than the summary later.
#[allow(dead_code)]
pub fn show(opened: &Opened) -> Option<&Show> {
    opened.engine.show()
}
