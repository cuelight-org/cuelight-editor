//! Where a layer sits on the canvas, as the engine places it now: the
//! map of the groups it is in, and its own place, turn and scale. A drag
//! on the stage is a distance on the canvas; this says what it is in the
//! numbers the layer writes.

use cuelight::{Engine, Transform};
use cuelight_core::{LayerKind, LayerPath, Property, root_layers};

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

    /// A distance on the canvas as a distance in the group the layer is
    /// in, which is what its `x` and `y` count in. `None` in a group
    /// scaled flat.
    pub fn in_parent(&self, by: [f64; 2]) -> Option<[f64; 2]> {
        Some(linear(self.parent).invert()?.apply(by))
    }

    /// A distance on the canvas along the layer's own axes, turned with
    /// it but not scaled: what its `scale_x` and `scale_y` stretch.
    fn in_own(&self, by: [f64; 2]) -> Option<[f64; 2]> {
        let turned = linear(self.parent).then(Transform::rotate(self.rotation));
        Some(turned.invert()?.apply(by))
    }

    /// The `scale_x` and `scale_y` that take the point `from` of the
    /// layer to `to`, both on the canvas, scaling round its pivot. With
    /// `keep`, both by the same factor, so the layer keeps its
    /// proportions. An axis the drag does not reach (the point was on it)
    /// keeps its scale.
    pub fn scaled(&self, from: [f64; 2], to: [f64; 2], keep: bool) -> Option<[f64; 2]> {
        let [px, py] = self.pivot();
        let [fx, fy] = self.in_own([from[0] - px, from[1] - py])?;
        let [tx, ty] = self.in_own([to[0] - px, to[1] - py])?;
        // Too near the pivot to say how far it went.
        const NEAR: f64 = 1e-6;
        if keep {
            let along = fx * fx + fy * fy;
            if along < NEAR {
                return None;
            }
            let factor = (tx * fx + ty * fy) / along;
            return Some([self.scale_x * factor, self.scale_y * factor]);
        }
        let factor = |from: f64, to: f64| if from.abs() < NEAR { 1.0 } else { to / from };
        Some([self.scale_x * factor(fx, tx), self.scale_y * factor(fy, ty)])
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
    fn a_corner_dragged_out_scales_round_the_pivot() {
        let engine = engine(
            r##"{"name": "r", "type": "shape", "x": 10, "y": 10, "shape": {"rect": [0, 0, 20, 10]}, "fill": "#FFFFFF"}"##,
        );
        let placed = placement(&engine, &LayerPath::new(Root::Show, [0])).unwrap();
        let scaled = placed.scaled([30.0, 20.0], [50.0, 25.0], false).unwrap();
        assert!(close(scaled, [2.0, 1.5]), "{scaled:?}");
        // Kept in proportion: the drag's reach along the corner.
        let kept = placed.scaled([30.0, 20.0], [50.0, 30.0], true).unwrap();
        assert!(close(kept, [2.0, 2.0]), "{kept:?}");
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
