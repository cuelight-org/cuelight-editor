//! A layer or a group alone: a show made of the picked subtree, played
//! on its own clock, with the triggers it listens to at hand.
//!
//! The solo's show is the original with its layers and scenes swapped
//! for the subtree: same size, background, fonts, variables and values,
//! and the same files, so its assets load as the show's do. The subtree
//! keeps its place on the canvas: each group above it comes along as a
//! plain group that only places it (its x, y, scale and rotation), with
//! nothing else of it, no timelines, bindings, clip or opacity. The
//! subtree is then the only child of the only child of the show, so it
//! is at `[0, 0, ...]` in the solo.
//!
//! A part of an artwork does not stand alone: soloing one solos its
//! artwork.

use std::collections::{BTreeMap, BTreeSet};

use cuelight::Engine;
use cuelight_core::{LayerPath, Root, Show};
use cuelight_loader::Options;
use serde_json::{Map, Value};

use crate::assets::{Asset, Kind};
use crate::session::{Instant, Session, lock};
use crate::tree;

/// The extension of a packed show, which a solo is saved as.
pub const PACK_EXTENSION: &str = cuelight_loader::PACK_EXTENSION;

/// What places a group's children, which a wrapper keeps of it.
const PLACING: [&str; 6] = ["x", "y", "rotation", "scale", "scale_x", "scale_y"];

/// A subtree playing alone.
pub struct Solo {
    /// The soloed layer, as the show's tree addresses it.
    pub path: LayerPath,
    /// Where it is in the show, in words: `picture/wolf`.
    pub place: String,
    /// The same layer in the solo's own show.
    pub inner: LayerPath,
    /// The solo's show as last loaded.
    pub document: Value,
    /// What plays it: its own engine and clock.
    pub session: Session,
    /// The triggers the subtree listens to or its presses fire.
    pub triggers: Vec<String>,
}

/// The layer a solo of `path` stands on: the layer itself, or for a
/// part of an artwork, the artwork.
pub fn soloed(show: &Show, path: &LayerPath) -> Option<LayerPath> {
    let layer = tree::layer(show, path)?;
    if matches!(layer.kind, cuelight_core::LayerKind::Part { .. }) {
        let mut up = path.indices.clone();
        up.pop();
        return (!up.is_empty()).then(|| LayerPath::new(path.root, up));
    }
    Some(path.clone())
}

/// The show of the layer at `path` alone, from `document`, the show as
/// written, and where the layer is in it. `None` when the document has
/// no layer there.
pub fn show_of(document: &Value, path: &LayerPath) -> Option<(Value, LayerPath)> {
    let mut layers = match path.root {
        Root::Show => document.get("layers")?,
        Root::Scene(i) => document.get("scenes")?.get(i)?.get("layers")?,
    };
    let mut chain = Vec::new();
    for (depth, &i) in path.indices.iter().enumerate() {
        let layer = layers.get(i)?;
        chain.push(layer);
        if depth + 1 < path.indices.len() {
            layers = layer.get("children").or_else(|| layer.get("parts"))?;
        }
    }
    let (layer, above) = chain.split_last()?;
    let mut wrapped = (*layer).clone();
    for ancestor in above.iter().rev() {
        let mut group = Map::new();
        group.insert("name".to_owned(), ancestor.get("name")?.clone());
        group.insert("type".to_owned(), Value::from("group"));
        for key in PLACING {
            if let Some(value) = ancestor.get(key) {
                group.insert(key.to_owned(), value.clone());
            }
        }
        group.insert("children".to_owned(), Value::Array(vec![wrapped]));
        wrapped = Value::Object(group);
    }
    let mut show = document.as_object()?.clone();
    show.remove("scenes");
    show.insert("layers".to_owned(), Value::Array(vec![wrapped]));
    let inner = LayerPath::new(Root::Show, vec![0; path.indices.len()]);
    Some((Value::Object(show), inner))
}

/// The triggers a solo's show listens to, and those its presses fire.
pub fn triggers_of(show: &Show) -> Vec<String> {
    let mut out: BTreeSet<String> = show.triggers();
    let inputs = crate::inputs::Inputs::of(show);
    out.extend(inputs.pressable.into_iter().map(|(_, trigger)| trigger));
    out.into_iter().collect()
}

impl Solo {
    /// Solo the layer at `path` of `show`, written as `document`, with the
    /// show's `files` for its assets and the lengths of its `sounds`.
    pub fn open(
        files: &BTreeMap<String, Vec<u8>>,
        document: &Value,
        show: &Show,
        path: &LayerPath,
        sounds: &[(String, f64)],
    ) -> Result<Self, String> {
        let path = soloed(show, path).ok_or("the show has no layer there")?;
        let (solo, inner) = show_of(document, &path).ok_or("the show has no layer there")?;
        let text = text_of(&solo)?;
        let mut files = files.clone();
        files.insert("show.json".to_owned(), text.into_bytes());
        let mut engine = Engine::new();
        cuelight_loader::load_from_memory_with(&mut engine, &files, &Options::lenient())
            .map_err(|e| e.to_string())?;
        for (name, length) in sounds {
            // A sound the solo does not play is not its business.
            let _ = engine.set_sound(name, *length);
        }
        let triggers = engine.show().map(triggers_of).unwrap_or_default();
        Ok(Self {
            place: tree::describe(show, &path),
            path,
            inner,
            document: solo,
            session: Session::new(engine, None),
            triggers,
        })
    }

    /// The show was edited: load the subtree as it is now, at the solo's
    /// playhead. An error when the show has no layer at the solo's path
    /// any more, or the solo does not load.
    pub fn reload(&mut self, document: &Value, now: Instant) -> Result<(), String> {
        let (solo, inner) =
            show_of(document, &self.path).ok_or("the soloed layer is gone from the show")?;
        let text = text_of(&solo)?;
        self.session
            .reload(&text, now)
            .map_err(|e| format!("the solo does not load ({e})"))?;
        self.document = solo;
        self.inner = inner;
        self.triggers = lock(&self.session.engine)
            .show()
            .map(triggers_of)
            .unwrap_or_default();
        Ok(())
    }

    /// The solo as a show of its own: its document, the show's fonts,
    /// and the other assets the subtree uses, by `library`.
    pub fn files(
        &self,
        files: &BTreeMap<String, Vec<u8>>,
        library: &[Asset],
    ) -> Result<BTreeMap<String, Vec<u8>>, String> {
        let mut out = BTreeMap::new();
        out.insert(
            "show.json".to_owned(),
            text_of(&self.document)?.into_bytes(),
        );
        for asset in library {
            let needed = matches!(asset.kind, Kind::Font)
                || asset.uses.iter().any(|u| tree::within(&u.path, &self.path));
            let Some(file) = asset.file.as_ref().filter(|_| needed) else {
                continue;
            };
            if let Some(bytes) = files.get(file) {
                out.insert(file.clone(), bytes.clone());
            }
        }
        Ok(out)
    }

    /// The solo as a packed show: [`Solo::files`] in one file.
    pub fn pack(
        &self,
        files: &BTreeMap<String, Vec<u8>>,
        library: &[Asset],
    ) -> Result<Vec<u8>, String> {
        cuelight_loader::pack_bytes(&self.files(files, library)?).map_err(|e| e.to_string())
    }

    /// The box round the subtree on the canvas as it is drawn now,
    /// `[x, y, width, height]`; `None` while it draws nothing.
    pub fn area(&self) -> Option<[f64; 4]> {
        let bounds = lock(&self.session.engine).bounds(&self.inner)?;
        let [x, y, w, h] = bounds.rect;
        let corners =
            [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| bounds.transform.apply(p));
        let (mut left, mut top) = (f64::INFINITY, f64::INFINITY);
        let (mut right, mut bottom) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for [cx, cy] in corners {
            left = left.min(cx);
            top = top.min(cy);
            right = right.max(cx);
            bottom = bottom.max(cy);
        }
        Some([left, top, right - left, bottom - top])
    }
}

/// A document as the solo writes it: readable, in the original's order.
fn text_of(document: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(document).map_err(|e| e.to_string())
}

/// The instants of a strip: `count` frames from `from`, `seconds` apart
/// in all, the last on `from + seconds`.
pub fn strip_times(from: f64, seconds: f64, count: usize) -> Vec<f64> {
    let step = if count > 1 {
        seconds / (count - 1) as f64
    } else {
        0.0
    };
    (0..count).map(|i| from + step * i as f64).collect()
}

/// The box of `boxes` taken together, grown by `margin` and kept inside
/// a canvas of `size`, in whole pixels: `[x, y, width, height]`. `None`
/// when nothing is left of it.
pub fn crop(boxes: &[[f64; 4]], margin: f64, size: [u32; 2]) -> Option<[u32; 4]> {
    let mut iter = boxes.iter();
    let &[x, y, w, h] = iter.next()?;
    let (mut left, mut top, mut right, mut bottom) = (x, y, x + w, y + h);
    for &[x, y, w, h] in iter {
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + w);
        bottom = bottom.max(y + h);
    }
    let [cw, ch] = size.map(f64::from);
    let left = (left - margin).floor().clamp(0.0, cw);
    let top = (top - margin).floor().clamp(0.0, ch);
    let right = (right + margin).ceil().clamp(0.0, cw);
    let bottom = (bottom + margin).ceil().clamp(0.0, ch);
    let (w, h) = (right - left, bottom - top);
    (w >= 1.0 && h >= 1.0).then_some([left as u32, top as u32, w as u32, h as u32])
}

/// Frames of RGBA8 pixels, each `width` wide, cut to `area` and laid
/// side by side, `columns` to a row: the strip as one picture, its
/// width, height and pixels. A frame too small for `area` leaves its
/// cell clear.
pub fn sheet(frames: &[(u32, Vec<u8>)], area: [u32; 4], columns: usize) -> (u32, u32, Vec<u8>) {
    let [ax, ay, aw, ah] = area.map(|n| n as usize);
    let columns = columns.clamp(1, frames.len().max(1));
    let rows = frames.len().div_ceil(columns).max(1);
    let (width, height) = (aw * columns, ah * rows);
    let mut out = vec![0u8; width * height * 4];
    for (i, (frame_width, pixels)) in frames.iter().enumerate() {
        let (cx, cy) = (i % columns * aw, i / columns * ah);
        let frame_width = *frame_width as usize;
        for row in 0..ah {
            let from = ((ay + row) * frame_width + ax) * 4;
            let to = ((cy + row) * width + cx) * 4;
            let (Some(line), Some(into)) = (
                pixels.get(from..from + aw * 4),
                out.get_mut(to..to + aw * 4),
            ) else {
                continue;
            };
            into.copy_from_slice(line);
        }
    }
    (width as u32, height as u32, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strip_ends_on_its_last_second() {
        assert_eq!(strip_times(1.0, 1.0, 3), [1.0, 1.5, 2.0]);
        assert_eq!(strip_times(2.0, 1.0, 1), [2.0]);
        assert!(strip_times(0.0, 1.0, 0).is_empty());
    }

    #[test]
    fn a_crop_takes_every_box_and_stays_on_the_canvas() {
        assert_eq!(
            crop(
                &[[10.0, 10.0, 5.0, 5.0], [12.0, 4.5, 10.0, 2.0]],
                2.0,
                [64, 32]
            ),
            Some([8, 2, 16, 15])
        );
        assert_eq!(
            crop(&[[-5.0, 30.0, 10.0, 10.0]], 0.0, [64, 32]),
            Some([0, 30, 5, 2])
        );
        assert_eq!(crop(&[[70.0, 0.0, 4.0, 4.0]], 0.0, [64, 32]), None);
        assert_eq!(crop(&[], 0.0, [64, 32]), None);
    }

    #[test]
    fn a_sheet_lays_the_cut_frames_in_rows() {
        // Two 2x2 frames, one colour each, cut to their right column.
        let frame = |v: u8| (2, vec![v; 2 * 2 * 4]);
        let (w, h, pixels) = sheet(&[frame(1), frame(2), frame(3)], [1, 0, 1, 2], 2);
        assert_eq!((w, h), (2, 4));
        let at = |x: usize, y: usize| pixels[(y * 2 + x) * 4];
        assert_eq!([at(0, 0), at(1, 0), at(0, 2), at(1, 2)], [1, 2, 3, 0]);
    }
}
