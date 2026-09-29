//! The show's assets as the library lists them: each by kind, with what
//! the engine knows about it and where the show uses it.

use std::collections::BTreeMap;
use std::sync::Arc;

use cuelight::Engine;
use cuelight_core::{DigitDisplay, FontStyle, Layer, LayerKind, LayerPath, ReelCells, Root, Show};

use crate::artwork::{self, Structure};

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

    /// The kind in a word, for an asset whose file does not say.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Image => "image",
            Kind::Vector => "vector",
            Kind::Font => "font",
            Kind::Sound => "sound",
            Kind::Video => "video",
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
    /// How large that file is, in bytes.
    pub bytes: Option<usize>,
    /// Pixels for an image, the artwork's own size for a vector.
    pub size: Option<[f64; 2]>,
    /// The SVG's bytes, for vector artwork: its thumbnail is drawn from
    /// them, never through the engine.
    pub svg: Option<Arc<[u8]>>,
    /// What vector artwork is made of: its element ids and the parts the
    /// show names of them.
    pub structure: Option<Structure>,
    /// Where the show uses it, in document order. Empty is worth a mark.
    pub uses: Vec<Use>,
}

impl Asset {
    /// The file's format, by its extension in lower case: `png`, `svg`.
    pub fn format(&self) -> Option<String> {
        let file = self.file.as_deref()?;
        let name = file.rsplit('/').next().unwrap_or(file);
        let (_, extension) = name.rsplit_once('.')?;
        Some(extension.to_ascii_lowercase())
    }

    /// What the library says of it in one line: its format (or its kind
    /// without a file) and how often the show uses it.
    pub fn summary(&self) -> String {
        let what = self.format().unwrap_or_else(|| self.kind.name().to_owned());
        format!("{what}, {}", times_used(self.uses.len()))
    }
}

/// How often something is used, in words: `unused`, `used once`,
/// `used 3 times`.
pub fn times_used(count: usize) -> String {
    match count {
        0 => "unused".to_owned(),
        1 => "used once".to_owned(),
        n => format!("used {n} times"),
    }
}

/// A file's size in words: bytes below a kilobyte, else KB or MB with
/// one decimal.
pub fn file_size(bytes: usize) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K {
        format!("{bytes} bytes")
    } else if b < K * K {
        format!("{:.1} KB", b / K)
    } else {
        format!("{:.1} MB", b / (K * K))
    }
}

/// One place in the show that names an asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    /// The layer, by its path: `group/dot`, or `scene play: out_tide`.
    pub place: String,
    /// The same layer, as the tree and the stage address it.
    pub path: LayerPath,
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
            bytes: None,
            size: engine
                .image(name)
                .map(|i| [f64::from(i.width), f64::from(i.height)]),
            svg: None,
            structure: None,
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
            bytes: None,
            size: engine.vector(name).map(|v| [v.width, v.height]),
            structure: engine
                .vector(name)
                .map(|v| artwork::structure(v, name, engine.show())),
            uses: take(Ref::Artwork(name.clone())),
        });
    }
    for name in &names.fonts {
        out.push(Asset {
            name: name.clone(),
            kind: Kind::Font,
            file: file_for(files, "assets/fonts", name),
            bytes: None,
            size: None,
            svg: None,
            structure: None,
            uses: take(Ref::Font(name.clone())),
        });
    }
    for name in &names.sounds {
        out.push(Asset {
            name: name.clone(),
            kind: Kind::Sound,
            file: file_for(files, "assets/sounds", name),
            bytes: None,
            size: None,
            svg: None,
            structure: None,
            uses: take(Ref::Sound(name.clone())),
        });
    }
    for name in &names.videos {
        out.push(Asset {
            name: name.clone(),
            kind: Kind::Video,
            file: file_for(files, "assets/videos", name),
            bytes: None,
            size: engine.video(name).map(|v| [v.width, v.height]),
            svg: None,
            structure: None,
            uses: take(Ref::Video(name.clone())),
        });
    }
    for asset in &mut out {
        asset.bytes = asset.file.as_ref().and_then(|f| files.get(f)).map(Vec::len);
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
    let mut at = Walk {
        root: Root::Show,
        indices: Vec::new(),
        styles: &show.fonts,
    };
    at.walk(&show.layers, "", &mut out);
    for (i, scene) in show.scenes.iter().enumerate() {
        at.root = Root::Scene(i);
        at.walk(&scene.layers, &format!("scene {}: ", scene.name), &mut out);
    }
    out
}

/// Where the walk is: under which root, down which indices.
struct Walk<'a> {
    root: Root,
    indices: Vec<usize>,
    styles: &'a BTreeMap<String, FontStyle>,
}

impl Walk<'_> {
    fn walk(&mut self, layers: &[Layer], prefix: &str, out: &mut BTreeMap<Ref, Vec<Use>>) {
        for (i, layer) in layers.iter().enumerate() {
            self.indices.push(i);
            self.layer(layer, prefix, out);
            self.indices.pop();
        }
    }

    fn layer(&mut self, layer: &Layer, prefix: &str, out: &mut BTreeMap<Ref, Vec<Use>>) {
        let styles = self.styles;
        let place = if prefix.is_empty() || prefix.ends_with(": ") {
            format!("{prefix}{}", layer.name)
        } else {
            format!("{prefix}/{}", layer.name)
        };
        let path = LayerPath::new(self.root, self.indices.clone());
        let mut note = |what: Ref, how: String| {
            out.entry(what).or_default().push(Use {
                place: place.clone(),
                path: path.clone(),
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
        self.walk(layer.children(), &place, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(file: Option<&str>, uses: usize) -> Asset {
        let used = Use {
            place: "a".to_owned(),
            path: LayerPath::new(Root::Show, [0]),
            how: "image layer".to_owned(),
        };
        Asset {
            name: "a".to_owned(),
            kind: Kind::Vector,
            file: file.map(str::to_owned),
            bytes: None,
            size: None,
            svg: None,
            structure: None,
            uses: vec![used; uses],
        }
    }

    #[test]
    fn the_library_line_says_the_format_and_how_often_it_is_used() {
        assert_eq!(
            asset(Some("assets/wolf.SVG"), 3).summary(),
            "svg, used 3 times"
        );
        assert_eq!(
            asset(Some("assets/wolf.svg"), 1).summary(),
            "svg, used once"
        );
        assert_eq!(asset(Some("assets/wolf.svg"), 0).summary(), "svg, unused");
        assert_eq!(asset(None, 0).summary(), "vector, unused");
        assert_eq!(asset(Some("assets/v.1/wolf"), 0).format(), None);
    }

    #[test]
    fn a_file_size_is_said_in_a_unit_that_reads() {
        assert_eq!(file_size(512), "512 bytes");
        assert_eq!(file_size(1536), "1.5 KB");
        assert_eq!(file_size(3 * 1024 * 1024), "3.0 MB");
    }

    /// Each use carries the layer's path, so the window can pick it: a
    /// group's child by its indices, a scene's layer under its scene.
    #[test]
    fn a_use_knows_the_layer_it_is_in() {
        let show: Show = serde_json::from_str(
            r##"{ "format": 1, "name": "t", "size": [8, 8], "layers": [
              { "name": "floor", "type": "shape", "shape": { "rect": [0, 0, 4, 4] }, "fill": "#FFFFFF" },
              { "name": "group", "type": "group", "children": [
                { "name": "dot", "type": "image", "image": "dot" } ] } ],
              "scenes": [ { "name": "play", "trigger": "play", "layers": [
                { "name": "big", "type": "image", "image": "dot" } ] } ] }"##,
        )
        .unwrap();
        let uses = uses_in(&show).remove(&Ref::Artwork("dot".to_owned()));
        let found: Vec<(String, LayerPath)> = uses
            .unwrap_or_default()
            .into_iter()
            .map(|u| (u.place, u.path))
            .collect();
        assert_eq!(
            found,
            [
                ("group/dot".to_owned(), LayerPath::new(Root::Show, [1, 0])),
                (
                    "scene play: big".to_owned(),
                    LayerPath::new(Root::Scene(0), [0])
                ),
            ]
        );
    }
}
