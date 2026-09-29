//! What a font looks like: a sample line and a specimen of every
//! printable character, at each size the show uses the font at.
//!
//! The text is drawn by the engine's own text path, in a show of its own
//! made for the purpose, so a bitmap font or a pixel font shows the very
//! pixels the stage draws. An outline font resolves to glyph outlines,
//! which only the renderer fills; for those the host draws the text
//! itself, in the family the file declares, and this module says which.

use std::collections::BTreeMap;
use std::sync::Arc;

use cuelight::{Engine, ResolvedShape};
use cuelight_core::{Scaling, Show};
use cuelight_loader::Options;

/// One way a show uses a font: a size and whether it is drawn as pixels,
/// with the font styles that use it so.
#[derive(Debug, Clone, PartialEq)]
pub struct Sizing {
    /// Pixels per em for an outline font; `None` for a bitmap font,
    /// which has one size of its own.
    pub size: Option<f64>,
    /// Drawn as exact pixels (`pixels: true`), the way a bitmap font is.
    pub pixels: bool,
    /// The font styles that use the font at this size, in name order.
    pub styles: Vec<String>,
}

impl Sizing {
    /// How a font no style uses is shown: a bitmap font at its own size,
    /// an outline font at 16 pixels per em as outlines.
    pub fn unused(outline: bool) -> Self {
        Sizing {
            size: outline.then_some(16.0),
            pixels: !outline,
            styles: Vec::new(),
        }
    }
}

/// Every distinct size the show's font styles use `font` at, in order of
/// the styles' names. Whether a style draws as pixels follows the
/// engine's rule: always for a bitmap font (a style without a size),
/// else what the style says, else whether the show renders on its own
/// pixel grid.
pub fn sizings(show: &Show, font: &str) -> Vec<Sizing> {
    let grid = matches!(show.output.scaling, Some(Scaling::PixelPerfect));
    let mut out: Vec<Sizing> = Vec::new();
    for (name, style) in &show.fonts {
        if style.file != font {
            continue;
        }
        let pixels = style.size.is_none() || style.pixels.unwrap_or(grid);
        match out
            .iter_mut()
            .find(|s| s.size == style.size && s.pixels == pixels)
        {
            Some(sizing) => sizing.styles.push(name.clone()),
            None => out.push(Sizing {
                size: style.size,
                pixels,
                styles: vec![name.clone()],
            }),
        }
    }
    out
}

/// Pixels the engine drew: RGBA8, row-major, white glyphs on nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}

/// How a line of text in the font came out.
#[derive(Debug, Clone, PartialEq)]
pub enum Drawn {
    /// A bitmap or pixel font: the pixels the stage would draw.
    Raster(Raster),
    /// An outline font: the stage fills its outlines through the
    /// renderer; the host draws the line itself in this family, `None`
    /// when the file names none.
    Outline(Option<Family>),
}

/// The face an outline font file declares: what a host asks its own
/// text system for to draw in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Family {
    pub name: String,
    /// 100 (thin) to 900 (black); 400 is regular, 700 bold.
    pub weight: u16,
    pub italic: bool,
}

/// A line of the specimen: what it says, and how it came out; `None`
/// when the font has none of its characters and nothing was drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub drawn: Option<Drawn>,
}

/// The line a font's row shows: enough to see the face.
pub const SAMPLE: &str = "The quick brown fox jumps over the lazy dog 0123456789";

/// Printable ASCII in rows of sixteen, each row labelled by the code of
/// its first character: what the specimen shows.
pub fn rows() -> Vec<(String, String)> {
    (0x20u8..0x7F)
        .collect::<Vec<_>>()
        .chunks(16)
        .map(|chunk| {
            let text: String = chunk.iter().map(|&c| c as char).collect();
            (format!("{:02X}", chunk[0]), text)
        })
        .collect()
}

/// [`SAMPLE`] in `font` at `sizing`, from the show's files.
pub fn sample(files: &BTreeMap<String, Vec<u8>>, font: &str, sizing: &Sizing) -> Option<Drawn> {
    draw(files, font, sizing, &[SAMPLE]).pop().flatten()
}

/// Every printable character in `font` at `sizing`, a [`Line`] per row
/// of [`rows`].
pub fn specimen(files: &BTreeMap<String, Vec<u8>>, font: &str, sizing: &Sizing) -> Vec<Line> {
    lines(files, font, sizing, &[]).1
}

/// Everything the library shows of a font: how the show uses it, its
/// sample line at the first of those sizes, and its specimen at each.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// At least one: [`Sizing::unused`] when no style uses the font.
    pub sizings: Vec<Sizing>,
    pub sample: Option<Drawn>,
    /// A specimen per sizing, in the same order.
    pub specimens: Vec<Vec<Line>>,
}

/// The [`Look`] of `font` in `show`; `outline` says whether it is an
/// outline font, which decides how an unused one is shown.
pub fn look(files: &BTreeMap<String, Vec<u8>>, show: &Show, font: &str, outline: bool) -> Look {
    let mut sizings = sizings(show, font);
    if sizings.is_empty() {
        sizings.push(Sizing::unused(outline));
    }
    let mut sample = None;
    let specimens = sizings
        .iter()
        .enumerate()
        .map(|(i, sizing)| {
            // The first sizing draws the sample with its specimen, in one
            // engine.
            let extra: &[&str] = if i == 0 { &[SAMPLE] } else { &[] };
            let (mut drawn, lines) = lines(files, font, sizing, extra);
            if i == 0 {
                sample = drawn.pop().flatten();
            }
            lines
        })
        .collect();
    Look {
        sizings,
        sample,
        specimens,
    }
}

/// The specimen's lines in `font` at `sizing`, and `extra` texts drawn
/// in the same pass, one entry each.
fn lines(
    files: &BTreeMap<String, Vec<u8>>,
    font: &str,
    sizing: &Sizing,
    extra: &[&str],
) -> (Vec<Option<Drawn>>, Vec<Line>) {
    let rows = rows();
    let mut texts: Vec<&str> = rows.iter().map(|(_, text)| text.as_str()).collect();
    texts.extend_from_slice(extra);
    let mut drawn = draw(files, font, sizing, &texts);
    drawn.resize(texts.len(), None);
    let extra = drawn.split_off(rows.len());
    let lines = drawn
        .into_iter()
        .zip(rows)
        .map(|(drawn, (_, text))| Line { text, drawn })
        .collect();
    (extra, lines)
}

/// Draw `texts` in `font` at `sizing` through the engine: a show of one
/// text layer per line, loaded with the show's font files, resolved
/// once. One entry per text, `None` for a line that drew nothing;
/// empty when the font cannot be loaded at all.
fn draw(
    files: &BTreeMap<String, Vec<u8>>,
    font: &str,
    sizing: &Sizing,
    texts: &[&str],
) -> Vec<Option<Drawn>> {
    let mut style = serde_json::json!({ "file": font, "color": "#FFFFFF" });
    // Only an outline font has a size, and only it can be asked for
    // pixels; a bitmap font is pixels already.
    if let Some(size) = sizing.size {
        style["size"] = serde_json::json!(size);
        style["pixels"] = serde_json::json!(sizing.pixels);
    }
    // Room for the widest line at the largest size; the rasters are
    // taken from the resolved layers, so the canvas only has to hold
    // them.
    let pitch = sizing.size.unwrap_or(64.0).max(8.0) * 2.0;
    let layers: Vec<serde_json::Value> = texts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            serde_json::json!({
                "name": format!("line_{i}"),
                "type": "text",
                "font": "specimen",
                "text": text,
                "x": 0,
                "y": (i as f64) * pitch,
            })
        })
        .collect();
    let show = serde_json::json!({
        "name": "specimen",
        "size": [4096, (texts.len() as f64 * pitch).max(pitch) as u32],
        "fonts": { "specimen": style },
        "layers": layers,
    });
    let mut folder: BTreeMap<String, Vec<u8>> = files
        .iter()
        .filter(|(path, _)| path.starts_with("assets/fonts/"))
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect();
    folder.insert("show.json".to_owned(), show.to_string().into_bytes());
    let mut engine = Engine::new();
    if cuelight_loader::load_from_memory_with(&mut engine, &folder, &Options::lenient()).is_err() {
        return Vec::new();
    }
    let Ok(items) = engine.resolved_layers() else {
        return Vec::new();
    };
    let family = engine
        .outline_fonts()
        .find(|(name, _)| *name == font)
        .and_then(|(_, bytes)| family(bytes));
    texts
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let name = format!("line_{i}");
            let item = items.iter().find(|item| item.name == name)?;
            match &item.shape {
                ResolvedShape::Bitmap { image, .. } => Some(Drawn::Raster(Raster {
                    width: image.width,
                    height: image.height,
                    pixels: image.pixels.clone(),
                })),
                ResolvedShape::GlyphRun { .. } => Some(Drawn::Outline(family.clone())),
                _ => None,
            }
        })
        .collect()
}

/// The face an outline font file declares: its family name in English
/// when it has one, the typographic family before the legacy one (as
/// font systems group faces), with its weight and slant.
pub fn family(bytes: &[u8]) -> Option<Family> {
    use skrifa::attribute::Style;
    use skrifa::string::StringId;
    use skrifa::{FontRef, MetadataProvider};
    let font = FontRef::new(bytes).ok()?;
    let name = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| {
            font.localized_strings(id)
                .english_or_first()
                .map(|s| s.to_string())
        })?;
    let attributes = font.attributes();
    Some(Family {
        name,
        weight: attributes.weight.value().round().clamp(1.0, 1000.0) as u16,
        italic: attributes.style != Style::Normal,
    })
}
