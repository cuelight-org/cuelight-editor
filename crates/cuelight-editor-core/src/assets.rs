//! The show's assets as the library lists them: each by kind, with what
//! the engine knows about it and where the show uses it.

use std::collections::BTreeMap;
use std::sync::Arc;

use cuelight::Engine;
use cuelight_core::{DigitDisplay, FontStyle, Layer, LayerKind, ReelCells, Show};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Image,
    Vector,
    Font,
    Sound,
    Video,
}

impl Kind {
    /// The heading the library lists this kind under. Images and vector
    /// artwork are one kind of layer, so they share one.
    pub fn heading(self) -> &'static str {
        match self {
            Kind::Image | Kind::Vector => "ARTWORK",
            Kind::Font => "FONTS",
            Kind::Sound => "SOUNDS",
            Kind::Video => "VIDEO",
        }
    }
}

/// One asset the show shipped and the engine registered.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub kind: Kind,
    /// The file's path within the show, when it came from one.
    pub file: Option<String>,
    /// Pixels for an image, the artwork's own size for a vector.
    pub size: Option<[f64; 2]>,
    /// The SVG's bytes, for vector artwork: its thumbnail is drawn from
    /// them, never through the engine.
    pub svg: Option<Arc<[u8]>>,
    /// Where the show uses it, in document order. Empty is worth a mark.
    pub uses: Vec<Use>,
}

/// One place in the show that names an asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    /// The layer, by its path: `group/dot`, or `scene play: out_tide`.
    pub place: String,
    /// What names it there: an image layer, text in a font style, ...
    pub how: String,
}

/// The names the loader registered, by kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    pub images: Vec<String>,
    pub vectors: Vec<String>,
    pub fonts: Vec<String>,
    pub sounds: Vec<String>,
    pub videos: Vec<String>,
}

/// The library: every registered asset, with its facts and its uses.
pub fn library(engine: &Engine, names: &Names, files: &BTreeMap<String, Vec<u8>>) -> Vec<Asset> {
    let mut uses = engine.show().map(uses_in).unwrap_or_default();
    let mut take = |what: Ref| uses.remove(&what).unwrap_or_default();
    let mut out = Vec::new();
    for name in &names.images {
        out.push(Asset {
            name: name.clone(),
            kind: Kind::Image,
            file: file_for(files, "assets", name),
            size: engine
                .image(name)
                .map(|i| [f64::from(i.width), f64::from(i.height)]),
            svg: None,
            uses: take(Ref::Artwork(name.clone())),
        });
    }
    for name in &names.vectors {
        let file = file_for(files, "assets", name);
        out.push(Asset {
            name: name.clone(),
            kind: Kind::Vector,
            svg: file
                .as_ref()
                .and_then(|f| files.get(f))
                .map(|bytes| Arc::from(bytes.as_slice())),
            file,
            size: engine.vector(name).map(|v| [v.width, v.height]),
            uses: take(Ref::Artwork(name.clone())),
        });
    }
    for name in &names.fonts {
        out.push(Asset {
            name: name.clone(),
            kind: Kind::Font,
            file: file_for(files, "assets/fonts", name),
            size: None,
            svg: None,
            uses: take(Ref::Font(name.clone())),
        });
    }
    for name in &names.sounds {
        out.push(Asset {
            name: name.clone(),
            kind: Kind::Sound,
            file: file_for(files, "assets/sounds", name),
            size: None,
            svg: None,
            uses: take(Ref::Sound(name.clone())),
        });
    }
    for name in &names.videos {
        out.push(Asset {
            name: name.clone(),
            kind: Kind::Video,
            file: file_for(files, "assets/videos", name),
            size: engine.video(name).map(|v| [v.width, v.height]),
            svg: None,
            uses: take(Ref::Video(name.clone())),
        });
    }
    out
}

/// The file under `dir` whose stem is `name`, whatever its extension.
fn file_for(files: &BTreeMap<String, Vec<u8>>, dir: &str, name: &str) -> Option<String> {
    let prefix = format!("{dir}/{name}.");
    files
        .keys()
        .find(|path| path.starts_with(&prefix) && !path[prefix.len()..].contains('/'))
        .cloned()
}

/// What a layer names: artwork by the name an image layer or a reel cell
/// gives, a font by its file (through a font style), a sound or a clip.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Ref {
    Artwork(String),
    Font(String),
    Sound(String),
    Video(String),
}

fn uses_in(show: &Show) -> BTreeMap<Ref, Vec<Use>> {
    let mut out = BTreeMap::new();
    walk(&show.layers, "", &show.fonts, &mut out);
    for scene in &show.scenes {
        walk(
            &scene.layers,
            &format!("scene {}: ", scene.name),
            &show.fonts,
            &mut out,
        );
    }
    out
}

fn walk(
    layers: &[Layer],
    prefix: &str,
    styles: &BTreeMap<String, FontStyle>,
    out: &mut BTreeMap<Ref, Vec<Use>>,
) {
    for layer in layers {
        let place = if prefix.is_empty() || prefix.ends_with(": ") {
            format!("{prefix}{}", layer.name)
        } else {
            format!("{prefix}/{}", layer.name)
        };
        let mut note = |what: Ref, how: String| {
            out.entry(what).or_default().push(Use {
                place: place.clone(),
                how,
            });
        };
        let font = |style: &str, note: &mut dyn FnMut(Ref, String), how: &str| {
            if let Some(found) = styles.get(style) {
                note(
                    Ref::Font(found.file.clone()),
                    format!("{how} in style {style}"),
                );
            }
        };
        match &layer.kind {
            LayerKind::Image { image, .. } => {
                note(Ref::Artwork(image.clone()), "image layer".into())
            }
            LayerKind::Text { font: style, .. } => font(style, &mut note, "text"),
            LayerKind::Digits {
                display: DigitDisplay::Reel(reel),
                ..
            } => {
                if let Some(style) = &reel.font {
                    font(style, &mut note, "reel");
                }
                if let Some(ReelCells::Vectors(names) | ReelCells::Images(names)) = &reel.cells {
                    for name in names {
                        note(Ref::Artwork(name.clone()), "reel cell".into());
                    }
                }
            }
            LayerKind::Audio { sound, .. } => {
                for name in &sound.0 {
                    note(Ref::Sound(name.clone()), "audio layer".into());
                }
            }
            LayerKind::Video { video, .. } => {
                for name in &video.0 {
                    note(Ref::Video(name.clone()), "video layer".into());
                }
            }
            _ => {}
        }
        // A group's children, and an artwork layer's parts (which name
        // nothing of their own).
        walk(layer.children(), &place, styles, out);
    }
}
