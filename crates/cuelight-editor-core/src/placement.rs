//! Where a layer sits on the canvas, as the engine places it now: the
//! map of the groups it is in, and its own place, turn and scale. A drag
//! on the stage is a distance on the canvas; this says what it is in the
//! numbers the layer writes.

use cuelight::{Engine, Transform};
use cuelight_core::{LayerKind, LayerPath, Property, Shape, root_layers};

/// A layer's place as the engine resolves it at the playhead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The map of the group it is in onto the canvas: the identity for a
    /// layer at the top.
    pub parent: Transform,
    pub x: f64,
    pub y: f64,
    /// Degrees, clockwise.
    pub rotation: f64,
    /// The uniform `scale`, which multiplies both of the others.
    pub scale: f64,
    pub scale_x: f64,
    pub scale_y: f64,
}

/// Where the layer at `path` is placed. `None` for a layer the show
/// does not have, or one inside something other than a group (an
/// artwork's part, placed in the artwork's own coordinates).
pub fn placement(engine: &Engine, path: &LayerPath) -> Option<Placement> {
    let core = engine.core();
    let show = engine.show()?;
    let mut layers = root_layers(show, path.root)?;
    let mut parent = Transform::IDENTITY;
    let (last, above) = path.indices.split_last()?;
    let mut indices = Vec::new();
    // Each group above puts its children at its place, turned and
    // scaled; a group has no box, so it turns round its own origin.
    for &i in above {
        indices.push(i);
        let layer = layers.get(i)?;
        let LayerKind::Group { children, .. } = &layer.kind else {
            return None;
        };
        let number = |property| core.number(path.root, layer, &indices, property);
        let scale = number(Property::Scale);
        parent = parent.then(
            Transform::translate(number(Property::X), number(Property::Y))
                .then(Transform::rotate(number(Property::Rotation)))
                .then(Transform::scale(
                    scale * number(Property::ScaleX),
                    scale * number(Property::ScaleY),
                )),
        );
        layers = children;
    }
    indices.push(*last);
    let layer = layers.get(*last)?;
    let number = |property| core.number(path.root, layer, &indices, property);
    Some(Placement {
        parent,
        x: number(Property::X),
        y: number(Property::Y),
        rotation: number(Property::Rotation),
        scale: number(Property::Scale),
        scale_x: number(Property::ScaleX),
        scale_y: number(Property::ScaleY),
    })
}

/// `transform` without its move: what it does to a distance.
fn linear(transform: Transform) -> Transform {
    let [a, b, c, d, ..] = transform.0;
    Transform([a, b, c, d, 0.0, 0.0])
}

impl Placement {
    /// Where the layer's `x`, `y` lands on the canvas: the point it
    /// turns and scales round.
    pub fn pivot(&self) -> [f64; 2] {
        self.parent.apply([self.x, self.y])
    }

    /// The map of a group's own space onto the canvas: what its children
    /// and its clip are placed by. A group turns round its own origin.
    pub fn group(&self) -> Transform {
        let scale = self.scale;
        self.parent.then(
            Transform::translate(self.x, self.y)
                .then(Transform::rotate(self.rotation))
                .then(Transform::scale(scale * self.scale_x, scale * self.scale_y)),
        )
    }

    /// A distance on the canvas as a distance in the group the layer is
    /// in, which is what its `x` and `y` count in. `None` in a group
    /// scaled flat.
    pub fn in_parent(&self, by: [f64; 2]) -> Option<[f64; 2]> {
        Some(linear(self.parent).invert()?.apply(by))
    }

    /// The `scale_x` and `scale_y` that take the point `from` of the
    /// layer to `to`, and the `x` and `y` that keep the point `opposite`
    /// where it is, all on the canvas: a corner dragged with the corner
    /// across from it pinned. With `keep`, both axes by the same factor,
    /// so the layer keeps its proportions. An axis the drag does not
    /// reach (`from` level with `opposite` on it) keeps its scale. The
    /// scales are to 3 decimals, and the place follows the rounded scales,
    /// so the pinned corner stays put. `None` in a group scaled flat, or
    /// for a layer scaled flat.
    pub fn scaled_about(
        &self,
        opposite: [f64; 2],
        from: [f64; 2],
        to: [f64; 2],
        keep: bool,
    ) -> Option<([f64; 2], [f64; 2])> {
        // Along the layer's own axes, turned with it but not scaled.
        let own = linear(self.parent).then(Transform::rotate(self.rotation));
        let back = own.invert()?;
        let [fx, fy] = back.apply([from[0] - opposite[0], from[1] - opposite[1]]);
        let [tx, ty] = back.apply([to[0] - opposite[0], to[1] - opposite[1]]);
        // Too near the pinned corner to say how far it went.
        const NEAR: f64 = 1e-6;
        let factor = if keep {
            let along = fx * fx + fy * fy;
            if along < NEAR {
                return None;
            }
            let k = (tx * fx + ty * fy) / along;
            [k, k]
        } else {
            let factor = |from: f64, to: f64| if from.abs() < NEAR { 1.0 } else { to / from };
            [factor(fx, tx), factor(fy, ty)]
        };
        let round = |v: f64| (v * 1000.0).round() / 1000.0 + 0.0;
        let scales = [
            round(self.scale_x * factor[0]),
            round(self.scale_y * factor[1]),
        ];
        if self.scale_x.abs() < NEAR || self.scale_y.abs() < NEAR {
            return None;
        }
        let factor = [scales[0] / self.scale_x, scales[1] / self.scale_y];
        // From the pinned corner to the pivot, stretched as the layer is,
        // gives where the pivot goes.
        let [px, py] = self.pivot();
        let [vx, vy] = back.apply([px - opposite[0], py - opposite[1]]);
        let [dx, dy] = own.apply([vx * factor[0], vy * factor[1]]);
        let place = self
            .parent
            .invert()?
            .apply([opposite[0] + dx, opposite[1] + dy]);
        Some((scales, place))
    }

    /// The `rotation` that turns the point `from` of the layer to lie
    /// towards `to` from its pivot, both on the canvas. A group flipped
    /// by a negative scale turns its children the other way.
    pub fn turned(&self, from: [f64; 2], to: [f64; 2]) -> f64 {
        let [px, py] = self.pivot();
        let angle = |[x, y]: [f64; 2]| (y - py).atan2(x - px).to_degrees();
        let [a, b, c, d, ..] = self.parent.0;
        let flipped = a * d - b * c < 0.0;
        let by = angle(to) - angle(from);
        // The short way round, so a drag across the left never jumps.
        let by = (by + 180.0).rem_euclid(360.0) - 180.0;
        self.rotation + if flipped { -by } else { by }
    }
}

/// The four corners of the box of the layer at `path` on the canvas,
/// top left first and round as its own space has them: turned with it
/// when it is turned.
///
/// The engine gives a group whose children are placed each their own way,
/// or that is clipped while turned, a box on the canvas that is not
/// turned. A group's box is made here instead, in its own space: round
/// its children's boxes, cut to its clip, then placed as the group is, so
/// a turned group's box turns with it and its corners are its own.
pub fn corners(engine: &Engine, path: &LayerPath) -> Option<[[f64; 2]; 4]> {
    let bounds = engine.bounds(path)?;
    let drawn = {
        let [x, y, w, h] = bounds.rect;
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| bounds.transform.apply(p))
    };
    let show = engine.show()?;
    let Some(layer) = crate::tree::layer(show, path) else {
        return Some(drawn);
    };
    let LayerKind::Group { children, clip, .. } = &layer.kind else {
        return Some(drawn);
    };
    let Some(placed) = placement(engine, path) else {
        return Some(drawn);
    };
    let own = placed.parent.then(
        Transform::translate(placed.x, placed.y)
            .then(Transform::rotate(placed.rotation))
            .then(Transform::scale(
                placed.scale * placed.scale_x,
                placed.scale * placed.scale_y,
            )),
    );
    let back = own.invert()?;
    // Round the children's corners, in the group's own space.
    let mut points: Vec<[f64; 2]> = (0..children.len())
        .filter_map(|i| {
            let mut child = path.clone();
            child.indices.push(i);
            let b = engine.bounds(&child)?;
            let [x, y, w, h] = b.rect;
            Some(
                [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
                    .map(|p| back.apply(b.transform.apply(p))),
            )
        })
        .flatten()
        .collect();
    let cut = match clip {
        Some(cuelight_core::Shape::Rect {
            rect: [x, y, w, h], ..
        }) => Some([*x, *y, x + w, y + h]),
        Some(cuelight_core::Shape::Circle {
            circle: [cx, cy, r],
        }) => Some([cx - r, cy - r, cx + r, cy + r]),
        _ => None,
    };
    if points.is_empty()
        && let Some([x0, y0, x1, y1]) = cut
    {
        points = vec![[x0, y0], [x1, y1]];
    }
    let [mut x0, mut y0, mut x1, mut y1] = points.iter().fold(
        [f64::MAX, f64::MAX, f64::MIN, f64::MIN],
        |[a, b, c, d], p| [a.min(p[0]), b.min(p[1]), c.max(p[0]), d.max(p[1])],
    );
    if let Some([cx0, cy0, cx1, cy1]) = cut {
        (x0, y0, x1, y1) = (x0.max(cx0), y0.max(cy0), x1.min(cx1), y1.min(cy1));
    }
    if !(x0 <= x1 && y0 <= y1) {
        return Some(drawn);
    }
    Some([[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| own.apply(p)))
}

/// A group's `clip` of the kinds the stage has handles for, in the
/// group's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipShape {
    /// `[x, y, width, height]`, and the corner radius it keeps.
    Rect { rect: [f64; 4], radius: f64 },
    /// `[cx, cy, radius]`.
    Circle([f64; 3]),
}

/// A handle of a group's clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipHandle {
    /// A corner or the middle of a side of a rect: on each axis -1 for
    /// its left or top edge, 1 for its right or bottom, 0 for neither.
    Side([i8; 2]),
    /// The point on a circle's rim right of its centre: its radius.
    Rim,
    /// A circle's centre: its place.
    Centre,
}

/// A group's clip and where it lands on the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clip {
    /// The group's own space onto the canvas.
    pub transform: Transform,
    pub shape: ClipShape,
}

/// The clip of the group at `path`. `None` for a layer that is not a
/// group, a group without one, and one clipped by a path, which has no
/// handles.
pub fn clip(engine: &Engine, path: &LayerPath) -> Option<Clip> {
    let show = engine.show()?;
    let layer = crate::tree::layer(show, path)?;
    let LayerKind::Group {
        clip: Some(shape), ..
    } = &layer.kind
    else {
        return None;
    };
    let shape = match shape {
        Shape::Rect { rect, radius } => ClipShape::Rect {
            rect: *rect,
            radius: radius.unwrap_or(0.0),
        },
        Shape::Circle { circle } => ClipShape::Circle(*circle),
        Shape::Path { .. } => return None,
    };
    Some(Clip {
        transform: placement(engine, path)?.group(),
        shape,
    })
}

impl Clip {
    /// Each handle and where it is on the canvas: a rect's corners and
    /// the middles of its sides, round from the top left; a circle's
    /// centre and the point on its rim right of it.
    pub fn handles(&self) -> Vec<(ClipHandle, [f64; 2])> {
        let on_canvas = |p| self.transform.apply(p);
        match self.shape {
            ClipShape::Rect {
                rect: [x, y, w, h], ..
            } => [
                [-1, -1],
                [0, -1],
                [1, -1],
                [1, 0],
                [1, 1],
                [0, 1],
                [-1, 1],
                [-1, 0],
            ]
            .into_iter()
            .map(|side: [i8; 2]| {
                let at = |s: i8, from: f64, size: f64| from + size * f64::from(s + 1) / 2.0;
                (
                    ClipHandle::Side(side),
                    on_canvas([at(side[0], x, w), at(side[1], y, h)]),
                )
            })
            .collect(),
            ClipShape::Circle([cx, cy, r]) => vec![
                (ClipHandle::Centre, on_canvas([cx, cy])),
                (ClipHandle::Rim, on_canvas([cx + r, cy])),
            ],
        }
    }

    /// Its middle on the canvas.
    pub fn centre(&self) -> [f64; 2] {
        match self.shape {
            ClipShape::Rect {
                rect: [x, y, w, h], ..
            } => self.transform.apply([x + w / 2.0, y + h / 2.0]),
            ClipShape::Circle([cx, cy, _]) => self.transform.apply([cx, cy]),
        }
    }

    /// The clip with `handle` dragged from `from` to `to`, both on the
    /// canvas, by whole units of the group. A rect's side or corner
    /// moves and the sides across stay; one dragged past the other
    /// turns the rect round. A circle's rim sets its radius, its centre
    /// moves it. `None` in a group scaled flat.
    pub fn dragged(&self, handle: ClipHandle, from: [f64; 2], to: [f64; 2]) -> Option<ClipShape> {
        let by = linear(self.transform)
            .invert()?
            .apply([to[0] - from[0], to[1] - from[1]]);
        let [dx, dy] = by.map(f64::round);
        Some(match (self.shape, handle) {
            (ClipShape::Rect { rect, radius }, ClipHandle::Side([sx, sy])) => {
                let [x, y, w, h] = rect;
                let edges = |s: i8, from: f64, size: f64, d: f64| {
                    let (mut low, mut high) = (from, from + size);
                    match s {
                        -1 => low += d,
                        1 => high += d,
                        _ => {}
                    }
                    (low.min(high), (high - low).abs())
                };
                let (x, w) = edges(sx, x, w, dx);
                let (y, h) = edges(sy, y, h, dy);
                ClipShape::Rect {
                    rect: [x, y, w, h],
                    radius,
                }
            }
            (ClipShape::Circle([cx, cy, r]), ClipHandle::Centre) => {
                ClipShape::Circle([cx + dx, cy + dy, r])
            }
            (ClipShape::Circle([cx, cy, r]), ClipHandle::Rim) => {
                // How far from the centre the rim point went, unrounded
                // until the end.
                let [ux, uy] = by;
                let reach = (r + ux).hypot(uy);
                ClipShape::Circle([cx, cy, r + (reach - r).round()])
            }
            _ => return None,
        })
    }
}

/// The lines a moving box snaps to, on the canvas: x for vertical ones,
/// y for horizontal ones.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lines {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

impl Lines {
    /// The canvas's edges and middle, and the edges of each of `boxes`,
    /// each `[x, y, width, height]` on the canvas.
    pub fn of(size: [f64; 2], boxes: &[[f64; 4]]) -> Self {
        let [w, h] = size;
        let mut lines = Lines {
            xs: vec![0.0, w / 2.0, w],
            ys: vec![0.0, h / 2.0, h],
        };
        for [x, y, w, h] in boxes {
            lines.xs.extend([*x, x + w]);
            lines.ys.extend([*y, y + h]);
        }
        lines
    }
}

/// What a box snapped to: how far it moved to line up, and the line it
/// lined up with on each axis, if any.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Snapped {
    pub by: [f64; 2],
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// Line up the box `[x, y, width, height]` with the nearest of `lines`
/// no further than `within`, by its edges or its middle, on each axis on
/// its own.
pub fn snap(rect: [f64; 4], lines: &Lines, within: f64) -> Snapped {
    let [x, y, w, h] = rect;
    let nearest = |own: [f64; 3], lines: &[f64]| {
        own.iter()
            .flat_map(|own| lines.iter().map(move |line| (line - own, *line)))
            .filter(|(by, _)| by.abs() <= within)
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
    };
    let sx = nearest([x, x + w / 2.0, x + w], &lines.xs);
    let sy = nearest([y, y + h / 2.0, y + h], &lines.ys);
    Snapped {
        by: [sx.map_or(0.0, |s| s.0), sy.map_or(0.0, |s| s.0)],
        x: sx.map(|s| s.1),
        y: sy.map(|s| s.1),
    }
}

/// The box round `corners` on the canvas, `[x, y, width, height]`.
pub fn around(corners: &[[f64; 2]]) -> Option<[f64; 4]> {
    let first = corners.first()?;
    let (mut min, mut max) = (*first, *first);
    for [x, y] in corners {
        min = [min[0].min(*x), min[1].min(*y)];
        max = [max[0].max(*x), max[1].max(*y)];
    }
    Some([min[0], min[1], max[0] - min[0], max[1] - min[1]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuelight_core::Root;

    fn engine(layers: &str) -> Engine {
        let mut engine = Engine::new();
        engine
            .load_show(&format!(
                r#"{{"format": 1, "name": "t", "size": [200, 100], "layers": [{layers}]}}"#
            ))
            .unwrap();
        engine
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    const TURNED_GROUP: &str = r##"{"name": "g", "type": "group", "x": 100, "y": 50, "rotation": 90, "scale": 2,
        "children": [{"name": "r", "type": "shape", "x": 5, "shape": {"rect": [0, 0, 10, 10]}, "fill": "#FFFFFF"}]}"##;

    #[test]
    fn a_drag_in_a_turned_scaled_group_is_counted_in_the_group() {
        let engine = engine(TURNED_GROUP);
        let placed = placement(&engine, &LayerPath::new(Root::Show, [0, 0])).unwrap();
        assert!(close(placed.pivot(), [100.0, 60.0]), "{:?}", placed.pivot());
        // Down the canvas is along the group's x, at half the distance.
        assert!(close(placed.in_parent([0.0, 10.0]).unwrap(), [5.0, 0.0]));
    }

    #[test]
    fn a_corner_dragged_out_scales_with_the_corner_across_pinned() {
        // A 20 x 10 rect from (10, 10) to (30, 20).
        let engine = engine(
            r##"{"name": "r", "type": "shape", "x": 10, "y": 10, "shape": {"rect": [0, 0, 20, 10]}, "fill": "#FFFFFF"}"##,
        );
        let placed = placement(&engine, &LayerPath::new(Root::Show, [0])).unwrap();
        // The bottom right, with the top left pinned: the place stays.
        let (scales, place) = placed
            .scaled_about([10.0, 10.0], [30.0, 20.0], [50.0, 25.0], false)
            .unwrap();
        assert!(close(scales, [2.0, 1.5]), "{scales:?}");
        assert!(close(place, [10.0, 10.0]), "{place:?}");
        // Kept in proportion: the drag's reach along the corner.
        let (kept, _) = placed
            .scaled_about([10.0, 10.0], [30.0, 20.0], [50.0, 30.0], true)
            .unwrap();
        assert!(close(kept, [2.0, 2.0]), "{kept:?}");
        // The top left, with the bottom right pinned: dragged up and left
        // by the rect's size, it doubles and its place moves with it.
        let (scales, place) = placed
            .scaled_about([30.0, 20.0], [10.0, 10.0], [-10.0, 0.0], false)
            .unwrap();
        assert!(close(scales, [2.0, 2.0]), "{scales:?}");
        assert!(close(place, [-10.0, 0.0]), "{place:?}");
        // The top right, a pixel from level with the pivot: dragged up by
        // a pixel, it grows by a pixel, not by a fifth.
        let (scales, place) = placed
            .scaled_about([10.0, 20.0], [30.0, 10.0], [30.0, 9.0], false)
            .unwrap();
        assert!(close(scales, [1.0, 1.1]), "{scales:?}");
        assert!(close(place, [10.0, 9.0]), "{place:?}");
    }

    #[test]
    fn a_turned_layer_scales_along_its_own_axes() {
        // Turned a quarter: its width runs down the canvas from (50, 10).
        let engine = engine(
            r##"{"name": "r", "type": "shape", "x": 50, "y": 10, "rotation": 90, "shape": {"rect": [0, 0, 20, 10]}, "fill": "#FFFFFF"}"##,
        );
        let placed = placement(&engine, &LayerPath::new(Root::Show, [0])).unwrap();
        // Its far corner is at (40, 30); the pivot's corner pinned.
        let (scales, place) = placed
            .scaled_about([50.0, 10.0], [40.0, 30.0], [40.0, 50.0], false)
            .unwrap();
        assert!(close(scales, [2.0, 1.0]), "{scales:?}");
        assert!(close(place, [50.0, 10.0]), "{place:?}");
    }

    #[test]
    fn a_handle_dragged_round_the_pivot_turns_the_layer() {
        let engine = engine(
            r##"{"name": "r", "type": "shape", "x": 10, "y": 10, "rotation": 10, "shape": {"rect": [0, 0, 20, 10]}, "fill": "#FFFFFF"}"##,
        );
        let placed = placement(&engine, &LayerPath::new(Root::Show, [0])).unwrap();
        let turned = placed.turned([20.0, 10.0], [10.0, 20.0]);
        assert!((turned - 100.0).abs() < 1e-9, "{turned}");
    }

    #[test]
    fn a_clip_sits_where_the_engine_cuts_the_group() {
        let engine = engine(
            r##"{"name": "g", "type": "group", "x": 100, "y": 50, "rotation": 90, "scale": 2,
                "clip": {"rect": [0, 0, 20, 10], "radius": 3},
                "children": [{"name": "r", "type": "shape", "shape": {"rect": [-50, -50, 100, 100]}, "fill": "#FFFFFF"}]}"##,
        );
        let g = LayerPath::new(Root::Show, [0]);
        let clip = clip(&engine, &g).unwrap();
        let bounds = engine.bounds(&g).unwrap();
        assert_eq!(bounds.rect, [0.0, 0.0, 20.0, 10.0]);
        let [x, y, w, h] = bounds.rect;
        let handles = clip.handles();
        assert_eq!(handles.len(), 8);
        assert!(close(handles[0].1, bounds.transform.apply([x, y])));
        assert!(close(handles[4].1, bounds.transform.apply([x + w, y + h])));
        // Its child has no clip, and a layer that is not a group neither.
        assert!(super::clip(&engine, &LayerPath::new(Root::Show, [0, 0])).is_none());
    }

    #[test]
    fn a_clip_corner_dragged_moves_its_sides_and_keeps_the_radius() {
        let engine = engine(
            r##"{"name": "g", "type": "group", "x": 100, "y": 50, "rotation": 90, "scale": 2,
                "clip": {"rect": [0, 0, 20, 10], "radius": 3}, "children": []}"##,
        );
        let clip = clip(&engine, &LayerPath::new(Root::Show, [0])).unwrap();
        // Down the canvas is along the group's x at half the distance,
        // left is along its y: the bottom right corner, by 10.3 down and
        // 4 left, is 5 further right and 2 further down, whole units.
        let dragged = clip.dragged(ClipHandle::Side([1, 1]), [80.0, 90.0], [76.0, 100.3]);
        assert_eq!(
            dragged,
            Some(ClipShape::Rect {
                rect: [0.0, 0.0, 25.0, 12.0],
                radius: 3.0
            })
        );
        // The left side past the right turns the rect round; the other
        // axis stays.
        let past = clip.dragged(ClipHandle::Side([-1, 0]), [100.0, 50.0], [100.0, 110.0]);
        assert_eq!(
            past,
            Some(ClipShape::Rect {
                rect: [20.0, 0.0, 10.0, 10.0],
                radius: 3.0
            })
        );
    }

    #[test]
    fn a_circle_clip_moves_by_its_centre_and_sizes_by_its_rim() {
        let engine = engine(
            r##"{"name": "g", "type": "group", "x": 50, "y": 50, "clip": {"circle": [0, 0, 20]}, "children": []}"##,
        );
        let clip = clip(&engine, &LayerPath::new(Root::Show, [0])).unwrap();
        assert_eq!(
            clip.handles(),
            vec![
                (ClipHandle::Centre, [50.0, 50.0]),
                (ClipHandle::Rim, [70.0, 50.0])
            ]
        );
        let moved = clip.dragged(ClipHandle::Centre, [50.0, 50.0], [53.4, 47.0]);
        assert_eq!(moved, Some(ClipShape::Circle([3.0, -3.0, 20.0])));
        // The rim's distance from the centre, whichever way it went.
        let sized = clip.dragged(ClipHandle::Rim, [70.0, 50.0], [50.0, 80.2]);
        assert_eq!(sized, Some(ClipShape::Circle([0.0, 0.0, 30.0])));
    }

    #[test]
    fn a_box_snaps_to_the_nearest_line_on_each_axis() {
        let lines = Lines::of([200.0, 100.0], &[[40.0, 0.0, 20.0, 20.0]]);
        // Its right edge 2 short of the other box's left; its middle 1
        // off the canvas's.
        let snapped = snap([18.0, 39.0, 20.0, 20.0], &lines, 3.0);
        assert_eq!(snapped.x, Some(40.0));
        assert_eq!(snapped.y, Some(50.0));
        assert!(close(snapped.by, [2.0, 1.0]));
        let far = snap([10.0, 30.0, 10.0, 10.0], &lines, 3.0);
        assert_eq!(far, Snapped::default());
    }
}
