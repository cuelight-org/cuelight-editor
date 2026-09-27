//! A show as the editor holds it once opened: the engine it loaded into
//! and a summary of what came with it.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use cuelight::{Engine, Layer, LayerKind, Show};

/// What was opened, where from, and what it contained.
pub struct Opened {
    /// Where it came from: a path on the desktop, a file name in the
    /// browser.
    pub source: String,
    pub engine: Engine,
    pub summary: Summary,
}

/// The facts about a show worth showing before there is a stage.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    pub name: String,
    pub format: u32,
    pub size: [u32; 2],
    /// Layers in the show and in its scenes, groups' children included.
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
    /// Everything the load had to say: the engine's warnings, assets the
    /// build could not read, font families the artwork asked for.
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
        let mut engine = Engine::new();
        let loaded =
            cuelight_loader::load(&mut engine, path).map_err(|e| OpenError::Load(e.to_string()))?;
        let mut summary = summarize(&engine);
        summary.images = loaded.images.len();
        summary.vectors = loaded.vectors.len();
        summary.fonts = loaded.fonts.len();
        summary.sounds = loaded.sounds.len();
        summary.videos = loaded.videos.len();
        summary.driver = loaded.driver.as_ref().map(|d| (d.steps.len(), d.looping));
        summary
            .problems
            .extend(loaded.skipped.iter().map(|p| format!("skipped {p}")));
        summary.problems.extend(
            loaded
                .missing_fonts
                .iter()
                .map(|f| format!("artwork asks for font {f:?}, which the show does not ship")),
        );
        Ok(Self {
            source: path.display().to_string(),
            engine,
            summary,
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
        let document = files
            .get("show.json")
            .ok_or_else(|| OpenError::Load(format!("{name} holds no show.json")))?;
        check_format(&String::from_utf8_lossy(document))?;
        let mut engine = Engine::new();
        let loaded = cuelight_loader::load_from_memory(&mut engine, &files)
            .map_err(|e| OpenError::Load(e.to_string()))?;
        let mut summary = summarize(&engine);
        summary.images = loaded.images.len();
        summary.vectors = loaded.vectors.len();
        summary.fonts = loaded.fonts.len();
        summary.sounds = loaded.sounds.len();
        summary.driver = loaded.driver.as_ref().map(|d| (d.steps.len(), d.looping));
        summary
            .problems
            .extend(loaded.skipped.iter().map(|p| format!("skipped {p}")));
        summary.problems.extend(
            loaded
                .missing_fonts
                .iter()
                .map(|f| format!("artwork asks for font {f:?}, which the show does not ship")),
        );
        Ok(Self {
            source: name.to_owned(),
            engine,
            summary,
        })
    }
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
    if found > cuelight::FORMAT {
        return Err(OpenError::NewerFormat {
            found,
            known: cuelight::FORMAT,
        });
    }
    Ok(())
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
        if let LayerKind::Group { children, .. } = &layer.kind {
            count(children, summary);
        }
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
