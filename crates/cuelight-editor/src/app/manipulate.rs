//! Moving, scaling and turning the picked layers on the stage: a drag of
//! the selection or one of its handles, and the arrow keys. What changes
//! is the base value, one undo step a drag or a key; where a timeline or
//! binding owns the property at the playhead, the inspector asks first.
//! A group's clip is reshaped by its own handles, which nothing else
//! owns.

use super::*;
use cuelight::Engine;
use cuelight_editor_core::document::{Part, Pointer};
use cuelight_editor_core::edit;
use cuelight_editor_core::placement::{self, Clip, ClipShape, Lines, Placement};
use serde_json::Value as Json;
use std::time::Duration;

/// How near a line a moving box snaps to it, in logical pixels.
const SNAP: f64 = 6.0;

/// One base value to write: a property of the layer at a path.
type Edit = (LayerPath, Property, Json);

/// A drag on the stage, from where it took hold to where it is now.
#[derive(Debug, Clone)]
pub(super) struct Grab {
    grip: Grip,
    from: [f64; 2],
    to: [f64; 2],
    held: Held,
    /// Each layer it changes, placed as it was when the drag began.
    layers: Vec<(LayerPath, Placement)>,
    /// The box round what moves, on the canvas, as it was when the drag
    /// began, and the lines it snaps to.
    rect: Option<[f64; 4]>,
    lines: Lines,
    /// How near a line snaps, in canvas pixels at the zoom it began at.
    within: f64,
    /// What owns a property it changes at the playhead, if anything: the
    /// drag then writes nothing, and asks when it lets go.
    owner: Option<String>,
    /// What the values were when it began, and what it wrote last.
    start: Vec<Edit>,
    applied: Vec<Edit>,
    /// The lines it lines up with now, which the stage draws.
    pub guides: [Option<f64>; 2],
    /// When it last wrote, and how long writing and reloading took.
    written: Option<(Instant, Duration)>,
    /// For a scale, the corner of the box across from the one grabbed,
    /// on the canvas: it stays where it is.
    opposite: Option<[f64; 2]>,
    /// The group's clip as it was when a drag of one of its handles
    /// began, and the shape it wrote last.
    clip: Option<Clip>,
    clipped: Option<ClipShape>,
}

/// The least time between two writes of a drag: what a screen of any
/// rate gets is about 30 a second.
const WRITE_EVERY: Duration = Duration::from_millis(33);

/// The properties a grip changes.
fn properties(grip: Grip) -> &'static [Property] {
    match grip {
        Grip::Move => &[Property::X, Property::Y],
        Grip::Scale => &[Property::ScaleX, Property::ScaleY, Property::X, Property::Y],
        Grip::Turn => &[Property::Rotation],
        Grip::Clip(_) => &[],
    }
}

/// Where in the document a group writes its clip's numbers, under
/// the group at `group`, and the numbers: `clip/rect` or
/// `clip/circle`. A rect's `radius` beside them stays as it is.
fn clip_numbers(group: &Pointer, shape: ClipShape) -> (Pointer, Json) {
    let (key, numbers) = match shape {
        ClipShape::Rect { rect, .. } => ("rect", rect.to_vec()),
        ClipShape::Circle(circle) => ("circle", circle.to_vec()),
    };
    (
        group
            .then(Part::Key("clip".to_owned()))
            .then(Part::Key(key.to_owned())),
        Json::Array(numbers.into_iter().map(|n| number(n, 3)).collect()),
    )
}

/// `value` as the document writes it: to `places` decimals, and as an
/// integer when it is a whole number, so `40` stays `40`.
fn number(value: f64, places: i32) -> Json {
    let factor = 10f64.powi(places);
    let value = (value * factor).round() / factor + 0.0;
    if value.fract() == 0.0 && value.abs() < 1e15 {
        Json::from(value as i64)
    } else {
        Json::from(value)
    }
}

/// Whether one of the two paths is the other or inside it.
fn related(a: &LayerPath, b: &LayerPath) -> bool {
    a.root == b.root && (a.indices.starts_with(&b.indices) || b.indices.starts_with(&a.indices))
}

/// How far an arrow key nudges, on the canvas: a pixel, ten with Shift.
pub(super) fn nudge_by(key: &str, shift: bool) -> Option<[f64; 2]> {
    let step = if shift { 10.0 } else { 1.0 };
    match key {
        "ArrowLeft" => Some([-step, 0.0]),
        "ArrowRight" => Some([step, 0.0]),
        "ArrowUp" => Some([0.0, -step]),
        "ArrowDown" => Some([0.0, step]),
        _ => None,
    }
}

/// The new place of a layer moved `by` on the canvas: in whole units of
/// its group, unless it lined up with a line, which is exact.
fn moved(placed: &Placement, by: [f64; 2], whole: bool) -> Option<[Json; 2]> {
    let [dx, dy] = placed.in_parent(by)?;
    Some(if whole {
        [
            number(placed.x + dx.round(), 3),
            number(placed.y + dy.round(), 3),
        ]
    } else {
        [number(placed.x + dx, 3), number(placed.y + dy, 3)]
    })
}

impl App {
    /// The picked layers a move moves: not one inside another picked,
    /// which moves with it.
    fn movable(&self) -> Vec<LayerPath> {
        self.selection
            .iter()
            .filter(|path| {
                !self.selection.iter().any(|other| {
                    other != *path
                        && related(other, path)
                        && other.indices.len() < path.indices.len()
                })
            })
            .cloned()
            .collect()
    }

    /// The layers a moving box lines up with: those at the top and those
    /// beside a moving one, by their boxes on the canvas.
    fn snap_boxes(&self, engine: &Engine, moving: &[LayerPath]) -> Vec<[f64; 4]> {
        let beside = |path: &LayerPath| {
            path.indices.len() == 1
                || moving.iter().any(|m| {
                    m.root == path.root
                        && m.indices.len() == path.indices.len()
                        && m.indices.split_last().map(|(_, up)| up)
                            == path.indices.split_last().map(|(_, up)| up)
                })
        };
        self.rows
            .iter()
            .filter_map(|row| match row {
                Row::Layer { path, .. } => Some(path),
                Row::Root { .. } => None,
            })
            .filter(|path| beside(path) && !moving.iter().any(|m| related(m, path)))
            .filter_map(|path| placement::corners(engine, path))
            .filter_map(|corners| placement::around(&corners))
            .collect()
    }

    /// A drag on the stage began: hold the layers it changes where they
    /// are, and open its undo step, or say what owns them.
    pub(super) fn grab(&mut self, grip: Grip, at: [f64; 2]) -> Task<Message> {
        let picked = match grip {
            Grip::Move => self.movable(),
            Grip::Scale | Grip::Turn | Grip::Clip(_) => {
                self.selection.last().cloned().into_iter().collect()
            }
        };
        let Some(session) = &self.session else {
            return Task::none();
        };
        let engine = lock(&session.engine);
        let layers: Vec<(LayerPath, Placement)> = picked
            .iter()
            .filter_map(|path| Some((path.clone(), placement::placement(&engine, path)?)))
            .collect();
        let (rect, lines) = match (grip, engine.show()) {
            (Grip::Move, Some(show)) => {
                let corners: Vec<[f64; 2]> = picked
                    .iter()
                    .filter_map(|path| placement::corners(&engine, path))
                    .flatten()
                    .collect();
                let size = show.size.map(f64::from);
                (
                    placement::around(&corners),
                    Lines::of(size, &self.snap_boxes(&engine, &picked)),
                )
            }
            _ => (None, Lines::default()),
        };
        // The corner nearest where the scale was grabbed is the one
        // dragged; the one across from it stays.
        let opposite = match (grip, picked.first()) {
            (Grip::Scale, Some(path)) => placement::corners(&engine, path).and_then(|corners| {
                let distance = |p: &[f64; 2]| (p[0] - at[0]).hypot(p[1] - at[1]);
                let (nearest, _) = corners
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))?;
                corners.get((nearest + 2) % 4).copied()
            }),
            _ => None,
        };
        let clip = match grip {
            Grip::Clip(_) => picked
                .first()
                .and_then(|path| placement::clip(&engine, path)),
            _ => None,
        };
        drop(engine);
        if layers.is_empty() {
            self.status = "this layer is not placed on the stage".to_owned();
            return Task::none();
        }
        if matches!(grip, Grip::Clip(_)) && clip.is_none() {
            self.status = "this group has no clip with handles".to_owned();
            return Task::none();
        }
        // Asked before the engine is held again.
        let owner = layers.iter().find_map(|(path, _)| {
            properties(grip)
                .iter()
                .find_map(|property| self.owner(path, *property))
        });
        let mut grab = Grab {
            grip,
            from: at,
            to: at,
            held: Held {
                shift: false,
                ctrl: true,
            },
            layers,
            rect,
            lines,
            within: SNAP / f64::from(self.scale().max(f32::EPSILON)),
            owner,
            start: Vec::new(),
            applied: Vec::new(),
            guides: [None, None],
            written: None,
            opposite,
            clip,
            clipped: None,
        };
        // Where it starts, unsnapped: what a drag that changed nothing
        // ends on.
        grab.start = grab.edits().0;
        grab.held.ctrl = false;
        match &grab.owner {
            Some(owner) => {
                let names: Vec<String> = properties(grip)
                    .iter()
                    .map(|p| tree::property_name(*p))
                    .collect();
                self.status = format!(
                    "{} set by {owner} at the playhead: let go to choose",
                    names.join(" and ")
                );
            }
            None => {
                if let Some(document) = &mut self.document {
                    document.begin_step();
                }
            }
        }
        self.grab = Some(grab);
        Task::none()
    }

    /// The pointer moved while dragging; the layers follow on the next
    /// frame.
    pub(super) fn drag(&mut self, at: [f64; 2], held: Held) -> Task<Message> {
        if let Some(grab) = &mut self.grab {
            grab.to = at;
            grab.held = held;
        }
        Task::none()
    }

    /// A frame while dragging: write where the pointer has the layers
    /// now, if that is not what was written last. The audit waits for the
    /// drag to end; a drag on what something else owns writes nothing.
    ///
    /// A write reloads the show, so it waits a while after the last: at
    /// least [`WRITE_EVERY`], and at least as long as that one took, so
    /// a heavy show keeps up with the pointer by writing less often.
    pub(super) fn drag_apply(&mut self) -> Task<Message> {
        if self.drag_waits() {
            return Task::none();
        }
        self.drag_write()
    }

    /// Whether a drag's next write has to wait: no drag, or too soon
    /// after the last.
    pub(super) fn drag_waits(&self) -> bool {
        match &self.grab {
            None => true,
            Some(grab) => grab
                .written
                .is_some_and(|(at, took)| at.elapsed() < took.max(WRITE_EVERY)),
        }
    }

    /// Write where the pointer has the layers now, if that is not what
    /// was written last.
    fn drag_write(&mut self) -> Task<Message> {
        let began = Instant::now();
        let Some(grab) = &mut self.grab else {
            return Task::none();
        };
        if let (Grip::Clip(handle), Some(clip)) = (grab.grip, grab.clip) {
            let shape = clip.dragged(handle, grab.from, grab.to);
            if let (Some(shape), Some((path, _))) = (shape, grab.layers.first())
                && grab.clipped != Some(shape)
            {
                grab.clipped = Some(shape);
                let path = path.clone();
                self.write_clip(&path, shape);
            }
            return Task::none();
        }
        let (edits, guides) = grab.edits();
        grab.guides = guides;
        if edits == grab.applied {
            return Task::none();
        }
        grab.applied = edits.clone();
        if grab.owner.is_none() {
            self.write_all(&edits, false);
            if let Some(grab) = &mut self.grab {
                grab.written = Some((began, began.elapsed()));
            }
        }
        Task::none()
    }

    /// The drag let go: where it ends is where the layers stay, in one
    /// undo step; or the inspector asks, when something owns them.
    pub(super) fn release(&mut self) -> Task<Message> {
        // Where it is let go is written, however soon after the last.
        let _ = self.drag_write();
        let Some(grab) = self.grab.take() else {
            return Task::none();
        };
        if let Some(owner) = grab.owner {
            if grab.applied != grab.start
                && let Some(path) = self.selection.last().cloned()
            {
                self.owned = Some(editing::Owned {
                    path,
                    edits: grab.applied,
                    owner,
                });
            }
            return Task::none();
        }
        self.finish(&grab.applied);
        Task::none()
    }

    /// An arrow key: move the picked layers a pixel on the canvas, ten
    /// with Shift, as one undo step.
    pub(super) fn nudge(&mut self, by: [f64; 2]) -> Task<Message> {
        self.commit_typed(None);
        let picked = self.movable();
        let Some(session) = &self.session else {
            return Task::none();
        };
        let engine = lock(&session.engine);
        let edits: Vec<Edit> = picked
            .iter()
            .filter_map(|path| {
                let placed = placement::placement(&engine, path)?;
                let [x, y] = moved(&placed, by, false)?;
                Some([
                    (path.clone(), Property::X, x),
                    (path.clone(), Property::Y, y),
                ])
            })
            .flatten()
            .collect();
        drop(engine);
        let owner = edits
            .iter()
            .find_map(|(path, property, _)| self.owner(path, *property));
        if let Some(owner) = owner {
            if let Some(path) = self.selection.last().cloned() {
                self.owned = Some(editing::Owned { path, edits, owner });
            }
            return Task::none();
        }
        if let Some(document) = &mut self.document {
            document.begin_step();
        }
        self.write_all(&edits, false);
        self.finish(&edits);
        Task::none()
    }

    /// Write each of `edits` into the document, then load it once.
    fn write_all(&mut self, edits: &[Edit], audit: bool) {
        let at: Vec<_> = edits
            .iter()
            .map(|(path, _, _)| self.layer_pointer(path))
            .collect();
        let Some(document) = &mut self.document else {
            return;
        };
        for ((_, property, value), at) in edits.iter().zip(at) {
            let Some(at) = at else {
                self.status =
                    "the document does not have this layer where the show does".to_owned();
                continue;
            };
            if let Err(error) = edit::set(document, &at, *property, value.clone()) {
                self.status = format!("could not edit: {error}");
                return;
            }
        }
        let text = document.text();
        match self.reload_text(&text, audit) {
            Ok(()) => {
                let said: Vec<String> = edits
                    .iter()
                    .filter(|(path, ..)| self.selection.last() == Some(path))
                    .map(|(_, property, value)| {
                        format!("{} = {value}", tree::property_name(*property))
                    })
                    .collect();
                self.status = said.join(", ");
            }
            Err(error) => self.status = error,
        }
    }

    /// Write the clip of the group at `path` into the document, then
    /// load it.
    fn write_clip(&mut self, path: &LayerPath, shape: ClipShape) {
        let Some(group) = self.layer_pointer(path) else {
            self.status = "the document does not have this layer where the show does".to_owned();
            return;
        };
        let Some(document) = &mut self.document else {
            return;
        };
        let (at, numbers) = clip_numbers(&group, shape);
        if let Err(error) = document.set(&at, numbers.clone()) {
            self.status = format!("could not edit: {error}");
            return;
        }
        let text = document.text();
        match self.reload_text(&text, false) {
            Ok(()) => self.status = format!("clip = {numbers}"),
            Err(error) => self.status = error,
        }
    }

    /// End the step a drag or a key opened: what ended on its default
    /// is taken out, in the same step, and the document is audited.
    fn finish(&mut self, edits: &[Edit]) {
        let at: Vec<_> = edits
            .iter()
            .map(|(path, _, _)| self.layer_pointer(path))
            .collect();
        if let Some(document) = &mut self.document {
            let mut changed = false;
            for ((_, property, _), at) in edits.iter().zip(at) {
                let Some(at) = at else {
                    continue;
                };
                let default = match (
                    document.get(&at),
                    edit::pointer(&at, *property).and_then(|here| document.get(&here)),
                ) {
                    (Some(layer), Some(now)) => {
                        edit::is_default(&layer.value(), *property, &now.value())
                    }
                    _ => false,
                };
                if default && edit::unset(document, &at, *property).is_ok() {
                    changed = true;
                }
            }
            if changed {
                let text = document.text();
                let _ = self.reload_text(&text, false);
            }
        }
        if let Some(document) = &mut self.document {
            document.end_step();
        }
        if let (Some(session), Some(document)) = (&mut self.session, &self.document) {
            session.audit(&document.text());
        }
    }
}

impl Grab {
    /// The values the drag has the layers at now, and the lines a move
    /// lined up with.
    fn edits(&self) -> (Vec<Edit>, [Option<f64>; 2]) {
        let (from, to) = (self.from, self.to);
        match self.grip {
            Grip::Move => {
                let raw = [to[0] - from[0], to[1] - from[1]];
                // Ctrl lets go of the snapping.
                let snapped = match self.rect {
                    Some([x, y, w, h]) if !self.held.ctrl => {
                        placement::snap([x + raw[0], y + raw[1], w, h], &self.lines, self.within)
                    }
                    _ => placement::Snapped::default(),
                };
                let by = [raw[0] + snapped.by[0], raw[1] + snapped.by[1]];
                let whole = snapped.x.is_none() && snapped.y.is_none();
                let edits = self
                    .layers
                    .iter()
                    .filter_map(|(path, placed)| {
                        let [x, y] = moved(placed, by, whole)?;
                        Some([
                            (path.clone(), Property::X, x),
                            (path.clone(), Property::Y, y),
                        ])
                    })
                    .flatten()
                    .collect();
                (edits, [snapped.x, snapped.y])
            }
            Grip::Scale => {
                let edits = self
                    .layers
                    .first()
                    .and_then(|(path, placed)| {
                        let ([sx, sy], [x, y]) =
                            placed.scaled_about(self.opposite?, from, to, self.held.shift)?;
                        Some(vec![
                            (path.clone(), Property::ScaleX, number(sx, 3)),
                            (path.clone(), Property::ScaleY, number(sy, 3)),
                            (path.clone(), Property::X, number(x, 2)),
                            (path.clone(), Property::Y, number(y, 2)),
                        ])
                    })
                    .unwrap_or_default();
                (edits, [None, None])
            }
            Grip::Turn => {
                let edits = self
                    .layers
                    .first()
                    .map(|(path, placed)| {
                        // Whole degrees; with Shift, steps of 15.
                        let step = if self.held.shift { 15.0 } else { 1.0 };
                        let turned = (placed.turned(from, to) / step).round() * step;
                        vec![(path.clone(), Property::Rotation, number(turned, 0))]
                    })
                    .unwrap_or_default();
                (edits, [None, None])
            }
            // Written on its own, as it is no property.
            Grip::Clip(_) => (Vec::new(), [None, None]),
        }
    }
}
