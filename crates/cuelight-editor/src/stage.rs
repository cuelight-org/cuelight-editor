//! The stage: the show as the players draw it, inside an iced widget.
//!
//! iced hands a shader widget its device and queue only in `prepare`, so
//! that is where the engine's presenter builds the frame and vello
//! renders it, into a texture of the widget's size. `render` then draws
//! that texture into the widget's rectangle of the window. Vello writes
//! through a storage binding, which a window's surface cannot be, hence
//! the texture in between; when the window is sRGB the texture is read
//! through an sRGB view, so the colours survive the round trip.
//!
//! A zoomed-in widget is larger than what its scrollable shows of it, so
//! the frame is only the part in view: [`Seen`] draws the shader over
//! that part alone, the presenter draws the rectangle of the canvas
//! under it into a frame its size, and `render` draws the slice of it
//! under the clip. Vello's compute renderer draws targets of at most
//! [`MAX_BINS`] bins of [`BIN`] pixels (linebender/vello#680, about
//! 4096 x 4096 in all), which now bounds the view rather than the zoom.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use cuelight::Engine;
use cuelight::render::Presenter;
use cuelight_core::LayerPath;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, renderer};
use iced::widget::shader::{self, Action, Shader, Viewport};
use iced::{Event, Length, Rectangle, Size, keyboard, mouse};

/// How a click picked: with Alt, the next layer down; with Shift, added
/// to the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pick {
    pub alt: bool,
    pub shift: bool,
}

/// The side of one of vello's coarse bins, in pixels.
pub const BIN: u32 = 256;
/// How many bins vello's compute renderer draws in one target.
pub const MAX_BINS: u32 = 256;

/// Whether vello draws a target of this size at all.
pub fn drawable(size: [u32; 2]) -> bool {
    size[0].div_ceil(BIN) * size[1].div_ceil(BIN) <= MAX_BINS
}

/// What a drag on the stage takes hold of: the picked layers, to move
/// them, or a handle of the last one picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grip {
    Move,
    /// A corner of its box: the drag scales it.
    Scale,
    /// The round knob over its box: the drag turns it.
    Turn,
}

/// The keys held during a drag: Shift keeps proportions or turns in
/// steps; Ctrl lets go of the snapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Held {
    pub shift: bool,
    pub ctrl: bool,
}

/// The widget's program: what to draw is whatever the engine shows now,
/// and a click on it is a press at a canvas point.
pub struct Stage<Message> {
    pub engine: Arc<Mutex<Engine>>,
    /// Changes when the show moved, so iced prepares a new frame.
    pub revision: u64,
    /// The layers drawn with a box round them.
    pub selection: Vec<LayerPath>,
    /// The lines a moving layer snapped to, x and y on the canvas.
    pub guides: [Option<f64>; 2],
    /// The message a click at a canvas point becomes: a pick of the
    /// layer there.
    pub on_pick: fn([f64; 2], Pick) -> Message,
    /// The message a Ctrl-click becomes: the show's own press.
    pub on_press: fn([f64; 2]) -> Message,
    /// The message a drag becomes once it is one: what it holds, from
    /// the canvas point it started at.
    pub on_grab: fn(Grip, [f64; 2]) -> Message,
    /// Where the drag is now on the canvas, and the keys held.
    pub on_drag: fn([f64; 2], Held) -> Message,
    /// The drag let go.
    pub on_release: fn() -> Message,
    /// The widget's whole box, as [`Seen`] last drew it.
    pub whole: Rc<Cell<Option<Rectangle>>>,
}

impl<Message> Stage<Message> {
    /// The widget, `width` by `height`, drawing only the part of it in
    /// view.
    pub fn widget(self, width: f32, height: f32) -> Seen<Message> {
        Seen {
            whole: self.whole.clone(),
            shader: Shader::new(self).width(width).height(height),
        }
    }
}

/// The stage's shader widget, drawn over only the part of it in view: a
/// scrollable draws its content with the rectangle it shows, and a
/// primitive the size of a zoomed-in show is more than the window's
/// render pass takes (16384 pixels a side).
pub struct Seen<Message> {
    shader: Shader<Message, Stage<Message>>,
    whole: Rc<Cell<Option<Rectangle>>>,
}

impl<Message> iced::advanced::widget::Meta for Seen<Message> {}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Seen<Message>
where
    Renderer: iced::advanced::Renderer,
    Shader<Message, Stage<Message>>: Widget<Message, Theme, Renderer>,
{
    fn size(&self) -> Size<Length> {
        Widget::<Message, Theme, Renderer>::size(&self.shader)
    }

    fn tag(&self) -> tree::Tag {
        Widget::<Message, Theme, Renderer>::tag(&self.shader)
    }

    fn state(&self) -> tree::State {
        Widget::<Message, Theme, Renderer>::state(&self.shader)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.shader.layout(tree, renderer, limits);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let whole = layout.bounds();
        let Some(seen) = viewport.intersection(&whole) else {
            return;
        };
        self.whole.set(Some(whole));
        let seen = Layout::new(seen.size()).move_to(seen.position());
        self.shader
            .draw(tree, renderer, theme, style, seen, cursor, viewport);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.shader
            .operate(tree, layout, viewport, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.shader
            .update(tree, event, layout, cursor, renderer, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.shader
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
}

/// The side of a corner handle, in logical pixels: the same at any zoom.
const HANDLE: f64 = 7.0;
/// How far the turning knob stands off the box's top edge, and its
/// radius, in logical pixels.
const KNOB_GAP: f64 = 22.0;
const KNOB: f64 = 4.5;
/// How near a handle the pointer takes hold of it, in logical pixels.
const REACH: f64 = 7.0;
/// How far the pointer goes before a press is a drag, in logical pixels.
const SLOP: f64 = 3.0;

/// What the stage keeps between events.
#[derive(Debug, Default)]
pub struct State {
    /// The modifiers held, which say what a click means.
    modifiers: keyboard::Modifiers,
    /// A press on the selection or a handle, until it lets go.
    holding: Option<Holding>,
}

/// A press that may become a drag.
#[derive(Debug, Clone, Copy)]
struct Holding {
    grip: Grip,
    /// Where it went down, in logical pixels in the widget.
    at: [f64; 2],
    /// The same on the canvas.
    canvas: [f64; 2],
    /// What it picks if it lets go without moving.
    pick: Pick,
    dragging: bool,
    /// Where the drag was last, on the canvas.
    last: [f64; 2],
}

/// Where the show lands in the widget, the way a player fits it into a
/// window: its top left and how many pixels a canvas pixel is.
#[derive(Debug, Clone, Copy)]
struct Fitted {
    x: f64,
    y: f64,
    sx: f64,
    sy: f64,
}

impl Fitted {
    /// The show fitted into a box `width` by `height`.
    fn new(engine: &Engine, width: f64, height: f64) -> Option<Self> {
        let show = engine.show()?;
        let (x, y, w, h) = cuelight::render::fit(
            show.size,
            [width.round() as u32, height.round() as u32],
            engine.scaling(),
            cuelight::render::Fit::Contain,
        );
        let [show_w, show_h] = show.size.map(|n| f64::from(n.max(1)));
        Some(Self {
            x,
            y,
            sx: w / show_w,
            sy: h / show_h,
        })
    }

    fn screen(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        [self.x + x * self.sx, self.y + y * self.sy]
    }

    /// A point in the widget on the canvas, beyond its edges too.
    fn canvas(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        [(x - self.x) / self.sx, (y - self.y) / self.sy]
    }
}

/// A box's handles on screen: its corners, which scale it, and the knob
/// over the middle of its top edge, which turns it. `unit` is how many
/// pixels a logical one is, so they keep their size at any zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Handles {
    corners: [[f64; 2]; 4],
    /// The middle of the top edge, where the knob's stem starts.
    top: [f64; 2],
    knob: [f64; 2],
}

fn handles(corners: [[f64; 2]; 4], unit: f64) -> Handles {
    let [a, b, ..] = corners;
    let top = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let middle = corners
        .iter()
        .fold([0.0, 0.0], |m, c| [m[0] + c[0] / 4.0, m[1] + c[1] / 4.0]);
    // Away from the middle, square to the top edge as it is turned; up
    // for a box too flat to say.
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy);
    let mut out = if length > 1e-9 {
        [dy / length, -dx / length]
    } else {
        [0.0, -1.0]
    };
    if (top[0] - middle[0]) * out[0] + (top[1] - middle[1]) * out[1] < 0.0 {
        out = [-out[0], -out[1]];
    }
    Handles {
        corners,
        top,
        knob: [
            top[0] + out[0] * KNOB_GAP * unit,
            top[1] + out[1] * KNOB_GAP * unit,
        ],
    }
}

/// Whether `point` is inside the box with these corners, turned or not.
fn inside(corners: &[[f64; 2]; 4], [x, y]: [f64; 2]) -> bool {
    let mut sides = [false, false];
    for (i, a) in corners.iter().enumerate() {
        let b = corners.get((i + 1) % 4).unwrap_or(a);
        let cross = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
        if cross > 0.0 {
            sides[0] = true;
        } else if cross < 0.0 {
            sides[1] = true;
        }
    }
    !(sides[0] && sides[1])
}

/// What a press at `point`, in logical pixels in the widget, takes hold
/// of: a handle of the one layer picked, or a picked layer's box.
fn grip_at(
    engine: &Engine,
    selection: &[LayerPath],
    fitted: &Fitted,
    point: [f64; 2],
) -> Option<Grip> {
    let boxes = boxes(engine, selection);
    if let ([one], [_]) = (boxes.as_slice(), selection) {
        let handles = handles(one.map(|c| fitted.screen(c)), 1.0);
        let near =
            |[x, y]: [f64; 2]| (x - point[0]).abs() <= REACH && (y - point[1]).abs() <= REACH;
        if near(handles.knob) {
            return Some(Grip::Turn);
        }
        if handles.corners.iter().any(|c| near(*c)) {
            return Some(Grip::Scale);
        }
    }
    let canvas = fitted.canvas(point);
    boxes
        .iter()
        .any(|corners| inside(corners, canvas))
        .then_some(Grip::Move)
}

impl<Message> shader::Program<Message> for Stage<Message> {
    type State = State;
    type Primitive = Frame;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let held = |modifiers: keyboard::Modifiers| Held {
            shift: modifiers.shift(),
            ctrl: modifiers.control(),
        };
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                // A key pressed or let go mid-drag counts at once.
                let holding = state.holding.filter(|h| h.dragging)?;
                Some(Action::publish((self.on_drag)(
                    holding.last,
                    held(*modifiers),
                )))
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                let engine = self.engine.lock().ok()?;
                let show = engine.show()?;
                let modifiers = state.modifiers;
                let pick = Pick {
                    alt: modifiers.alt(),
                    shift: modifiers.shift(),
                };
                // A press on the selection waits to see whether it is a
                // drag; a Ctrl-click is always the show's.
                let point = [f64::from(at.x), f64::from(at.y)];
                if !modifiers.control()
                    && let Some(fitted) =
                        Fitted::new(&engine, f64::from(bounds.width), f64::from(bounds.height))
                    && let Some(grip) = grip_at(&engine, &self.selection, &fitted, point)
                {
                    let canvas = fitted.canvas(point);
                    state.holding = Some(Holding {
                        grip,
                        at: point,
                        canvas,
                        pick,
                        dragging: false,
                        last: canvas,
                    });
                    return Some(Action::capture());
                }
                // The stage fits the show into its box the way a player
                // fits it into a window, so the same arithmetic maps a
                // point back.
                let target = [bounds.width.round() as u32, bounds.height.round() as u32];
                let point = cuelight::render::canvas_at(
                    show.size,
                    target,
                    engine.scaling(),
                    cuelight::render::Fit::Contain,
                    point,
                )?;
                let message = if modifiers.control() {
                    (self.on_press)(point)
                } else {
                    (self.on_pick)(point, pick)
                };
                Some(Action::publish(message).and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let holding = state.holding.as_mut()?;
                // Off the stage the pointer still drags, under whatever
                // it is over.
                let position = match cursor {
                    mouse::Cursor::Available(p)
                    | mouse::Cursor::Levitating(p)
                    | mouse::Cursor::Obstructed(p) => p,
                    mouse::Cursor::Unavailable => return None,
                };
                let point = [
                    f64::from(position.x - bounds.x),
                    f64::from(position.y - bounds.y),
                ];
                let engine = self.engine.lock().ok()?;
                let fitted =
                    Fitted::new(&engine, f64::from(bounds.width), f64::from(bounds.height))?;
                drop(engine);
                let canvas = fitted.canvas(point);
                if !holding.dragging {
                    let (dx, dy) = (point[0] - holding.at[0], point[1] - holding.at[1]);
                    if dx.hypot(dy) < SLOP {
                        return Some(Action::capture());
                    }
                    holding.dragging = true;
                    return Some(
                        Action::publish((self.on_grab)(holding.grip, holding.canvas)).and_capture(),
                    );
                }
                // The same place again is no move: nothing to say.
                if canvas == holding.last {
                    return Some(Action::capture());
                }
                holding.last = canvas;
                Some(Action::publish((self.on_drag)(canvas, held(state.modifiers))).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let holding = state.holding.take()?;
                if holding.dragging {
                    return Some(Action::publish((self.on_release)()).and_capture());
                }
                // Let go where it went down: a click, which picks; a
                // handle clicked does nothing.
                if holding.grip != Grip::Move {
                    return Some(Action::capture());
                }
                Some(Action::publish((self.on_pick)(holding.canvas, holding.pick)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &State, _cursor: mouse::Cursor, bounds: Rectangle) -> Frame {
        // `bounds` is the part in view; the show fills the whole box.
        let whole = self.whole.get().unwrap_or(bounds);
        Frame {
            engine: self.engine.clone(),
            revision: self.revision,
            selection: self.selection.clone(),
            guides: self.guides,
            whole: [
                whole.x - bounds.x,
                whole.y - bounds.y,
                whole.width,
                whole.height,
            ],
        }
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let shape = |grip| match grip {
            Grip::Move => mouse::Interaction::Move,
            Grip::Scale => mouse::Interaction::ResizingDiagonallyDown,
            Grip::Turn => mouse::Interaction::Grab,
        };
        if let Some(holding) = state.holding.filter(|h| h.dragging) {
            return match holding.grip {
                Grip::Turn => mouse::Interaction::Grabbing,
                grip => shape(grip),
            };
        }
        let Some(at) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let Ok(engine) = self.engine.lock() else {
            return mouse::Interaction::default();
        };
        let Some(show) = engine.show() else {
            return mouse::Interaction::default();
        };
        // A click picks, and the selection and its handles can be
        // dragged; only a Ctrl-click presses the show.
        if !state.modifiers.control() {
            return Fitted::new(&engine, f64::from(bounds.width), f64::from(bounds.height))
                .and_then(|fitted| {
                    grip_at(
                        &engine,
                        &self.selection,
                        &fitted,
                        [f64::from(at.x), f64::from(at.y)],
                    )
                })
                .map_or(mouse::Interaction::default(), shape);
        }
        // A pointer over something pressable says so.
        let target = [bounds.width.round() as u32, bounds.height.round() as u32];
        let pressable = cuelight::render::canvas_at(
            show.size,
            target,
            engine.scaling(),
            cuelight::render::Fit::Contain,
            [f64::from(at.x), f64::from(at.y)],
        )
        .is_some_and(|point| engine.pressed(point).is_some());
        if pressable {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// One frame to draw: a handle on the engine at a revision.
pub struct Frame {
    engine: Arc<Mutex<Engine>>,
    revision: u64,
    selection: Vec<LayerPath>,
    guides: [Option<f64>; 2],
    /// The widget's whole box, `[x, y, width, height]` in logical
    /// pixels from the top left of the part in view, which is what the
    /// frame is drawn over.
    whole: [f32; 4],
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Frame(revision {}, {} selected)",
            self.revision,
            self.selection.len()
        )
    }
}

/// The outline round each selected layer, as four corners on the
/// canvas: the box the engine draws it in and hit-tests presses against
/// (`Engine::bounds`), turned with the layer when it is turned. A layer
/// that draws nothing now (hidden, clipped away) has none.
fn boxes(engine: &Engine, selection: &[LayerPath]) -> Vec<[[f64; 2]; 4]> {
    selection
        .iter()
        .filter_map(|path| engine.bounds(path))
        .map(|bounds| corners(&bounds))
        .collect()
}

/// The four corners of a layer's box on the canvas, top left first and
/// round as its own space has them, turned with it when it is turned.
pub fn corners(bounds: &cuelight::LayerBounds) -> [[f64; 2]; 4] {
    let [x, y, w, h] = bounds.rect;
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| bounds.transform.apply(p))
}

/// Draw the selection's boxes over the presented show, with the handles
/// of the one picked when only one is, and the guides a move snapped to
/// across the canvas. `unit` is how many of the frame's pixels a logical
/// pixel is: the lines and handles keep their size at any zoom.
fn outline(
    scene: &mut vello::Scene,
    boxes: &[[[f64; 2]; 4]],
    with_handles: bool,
    guides: [Option<f64>; 2],
    fitted: &Fitted,
    show: [u32; 2],
    unit: f64,
) {
    use vello::kurbo::{Affine, BezPath, Circle, Line, Point, Rect, Stroke};
    let color = vello::peniko::Color::from_rgba8(0x5B, 0x8C, 0xFF, 0xFF);
    let white = vello::peniko::Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF);
    let guide = vello::peniko::Color::from_rgba8(0xFF, 0x4F, 0xA3, 0xFF);
    let point = |p: [f64; 2]| Point::new(p[0], p[1]);
    let line = Stroke::new(1.5 * unit);
    // The guides under the boxes, so the handles stay on top.
    let [w, h] = show.map(f64::from);
    let thin = Stroke::new(unit);
    if let [Some(x), _] = guides {
        let across = Line::new(point(fitted.screen([x, 0.0])), point(fitted.screen([x, h])));
        scene.stroke(&thin, Affine::IDENTITY, guide, None, &across);
    }
    if let [_, Some(y)] = guides {
        let across = Line::new(point(fitted.screen([0.0, y])), point(fitted.screen([w, y])));
        scene.stroke(&thin, Affine::IDENTITY, guide, None, &across);
    }
    for corners in boxes {
        let mut path = BezPath::new();
        for (i, corner) in corners.iter().enumerate() {
            let at = point(fitted.screen(*corner));
            if i == 0 {
                path.move_to(at);
            } else {
                path.line_to(at);
            }
        }
        path.close_path();
        scene.stroke(&line, Affine::IDENTITY, color, None, &path);
    }
    if let ([one], true) = (boxes, with_handles) {
        let handles = handles(one.map(|c| fitted.screen(c)), unit);
        scene.stroke(
            &line,
            Affine::IDENTITY,
            color,
            None,
            &Line::new(point(handles.top), point(handles.knob)),
        );
        let knob = Circle::new(point(handles.knob), KNOB * unit);
        scene.fill(
            vello::peniko::Fill::NonZero,
            Affine::IDENTITY,
            white,
            None,
            &knob,
        );
        scene.stroke(&line, Affine::IDENTITY, color, None, &knob);
        let half = HANDLE * unit / 2.0;
        for [x, y] in handles.corners {
            let square = Rect::new(x - half, y - half, x + half, y + half);
            scene.fill(
                vello::peniko::Fill::NonZero,
                Affine::IDENTITY,
                white,
                None,
                &square,
            );
            scene.stroke(&line, Affine::IDENTITY, color, None, &square);
        }
    }
}

impl shader::Primitive for Frame {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        pipeline.drawn = false;
        let gpu = pipeline.gpu.get_mut().unwrap_or_else(|e| e.into_inner());
        let Some(renderer) = gpu.renderer.as_mut() else {
            return;
        };
        // The frame covers the part in view out to whole physical
        // pixels, so each of its pixels is one of the window's.
        let scale = viewport.scale_factor();
        let (x0, y0) = ((bounds.x * scale).floor(), (bounds.y * scale).floor());
        let size = [
            (((bounds.x + bounds.width) * scale).ceil() - x0).max(1.0) as u32,
            (((bounds.y + bounds.height) * scale).ceil() - y0).max(1.0) as u32,
        ];
        pipeline.bounds = [x0, y0, size[0] as f32, size[1] as f32];
        // The whole widget in physical pixels, from the frame's top left.
        let [wx, wy, ww, wh] = [
            (bounds.x + self.whole[0]) * scale - x0,
            (bounds.y + self.whole[1]) * scale - y0,
            self.whole[2] * scale,
            self.whole[3] * scale,
        ]
        .map(f64::from);
        if !drawable(size) {
            if pipeline.target.take().is_some() {
                log::warn!(
                    "stage: {} x {} is more than vello draws, the stage stays blank",
                    size[0],
                    size[1]
                );
            }
            return;
        }
        if pipeline.target.as_ref().is_none_or(|t| t.size != size) {
            pipeline.target = Some(Target::new(
                device,
                &pipeline.layout,
                &pipeline.sampler,
                &pipeline.uniform,
                pipeline.srgb,
                size,
            ));
        }
        let Some(target) = pipeline.target.as_ref() else {
            return;
        };

        let presented = {
            let engine = match self.engine.lock() {
                Ok(engine) => engine,
                Err(_) => return,
            };
            let Some(show) = engine.show() else {
                return;
            };
            // Where the show lands in the whole widget, from the frame's
            // top left, and so the rectangle of the canvas in view.
            let (px, py, pw, ph) = cuelight::render::fit(
                show.size,
                [ww.round() as u32, wh.round() as u32],
                engine.scaling(),
                cuelight::render::Fit::Contain,
            );
            let (px, py) = (wx + px, wy + py);
            let [show_w, show_h] = show.size.map(f64::from);
            let (sx, sy) = (show_w / pw, show_h / ph);
            let view = [
                -px * sx,
                -py * sy,
                f64::from(size[0]) * sx,
                f64::from(size[1]) * sy,
            ];
            let mut presented = match gpu
                .presenter
                .present_view(&engine, device, queue, renderer, size, view)
            {
                Ok(presented) => presented,
                Err(error) => {
                    log::warn!("stage: cannot present the show: {error}");
                    return;
                }
            };
            if !self.selection.is_empty() || self.guides.iter().any(Option::is_some) {
                let fitted = Fitted {
                    x: px,
                    y: py,
                    sx: pw / show_w,
                    sy: ph / show_h,
                };
                outline(
                    &mut presented.scene,
                    &boxes(&engine, &self.selection),
                    self.selection.len() == 1,
                    self.guides,
                    &fitted,
                    show.size,
                    f64::from(scale),
                );
            }
            presented
        };
        if let Err(error) = renderer.render_to_texture(
            device,
            queue,
            &presented.scene,
            &target.storage,
            &vello::RenderParams {
                base_color: presented.base_color,
                width: size[0],
                height: size[1],
                antialiasing_method: vello::AaConfig::Area,
            },
        ) {
            log::warn!("stage: vello did not render: {error}");
            return;
        }
        pipeline.drawn = true;
    }

    fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip: &Rectangle<u32>,
    ) {
        let Some(frame) = pipeline.target.as_ref().filter(|_| pipeline.drawn) else {
            return;
        };
        // The slice of the frame under the clip, as texture coordinates.
        let [x, y, w, h] = pipeline.bounds;
        let u0 = (clip.x as f32 - x) / w;
        let v0 = (clip.y as f32 - y) / h;
        let rect = [
            u0,
            v0,
            u0 + clip.width as f32 / w,
            v0 + clip.height as f32 / h,
        ];
        let bytes: Vec<u8> = rect.iter().flat_map(|v| v.to_ne_bytes()).collect();
        pipeline.queue.write_buffer(&pipeline.uniform, 0, &bytes);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cuelight-stage-blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            clip.x as f32,
            clip.y as f32,
            clip.width as f32,
            clip.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
        pass.set_pipeline(&pipeline.blit);
        pass.set_bind_group(0, &frame.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// What lives on the GPU across frames: the vello renderer, the
/// presenter with its own targets, the texture between the two, and the
/// pipeline that draws that texture into the window.
pub struct Pipeline {
    /// The presenter and vello's renderer, neither of which is `Sync`,
    /// behind a lock the pipeline (which must be) opens in `prepare`.
    gpu: Mutex<Gpu>,
    blit: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The queue, for writing the blit's slice from `render`, which is
    /// handed an encoder only.
    queue: wgpu::Queue,
    /// Which slice of the frame the blit draws: `[u0, v0, u1, v1]`.
    uniform: wgpu::Buffer,
    /// The widget's whole box in physical pixels, `[x, y, width,
    /// height]`, as the last `prepare` saw it.
    bounds: [f32; 4],
    /// The window wants sRGB-encoded values decoded on the way in.
    srgb: bool,
    target: Option<Target>,
    /// The last `prepare` put a frame in `target`.
    drawn: bool,
}

struct Gpu {
    presenter: Presenter,
    /// `None` where vello cannot run: a device without compute, which is
    /// what a browser's WebGL2-level device is. The stage then stays
    /// blank and the log says why.
    renderer: Option<vello::Renderer>,
}

struct Target {
    size: [u32; 2],
    /// The view vello writes: `Rgba8Unorm`, as a storage binding.
    storage: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

/// The blit shader, its checkerboard greys in the window's encoding:
/// linear values for an sRGB window, which encodes them on the way out.
fn blit(srgb: bool) -> String {
    let grey = |level: f32| {
        let level = if srgb { level.powf(2.2) } else { level };
        format!("{level:.4}")
    };
    BLIT.replace("CHECKER_LIGHT", &grey(0.30))
        .replace("CHECKER_DARK", &grey(0.22))
}

const BLIT: &str = r#"
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
// The slice of the frame under the viewport: u0, v0, u1, v1.
@group(0) @binding(2) var<uniform> slice: vec4<f32>;

struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }

// One triangle over the whole viewport; the viewport is the visible
// part of the widget, and gets the frame's slice under it.
@vertex fn vs(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    let uv = mix(slice.xy, slice.zw, corner);
    return Vertex(vec4<f32>(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0), uv);
}

// The frame over a checkerboard, so what the show leaves see-through
// (a background with alpha) reads as such; vello's pixels are not
// premultiplied. Always opaque: the window is never see-through.
@fragment fn fs(vertex: Vertex) -> @location(0) vec4<f32> {
    let pixel = textureSample(frame, frame_sampler, vertex.uv);
    let cell = vec2<u32>(vertex.position.xy) / 8u;
    let light = ((cell.x + cell.y) & 1u) == 0u;
    let checker = vec3<f32>(select(CHECKER_DARK, CHECKER_LIGHT, light));
    return vec4<f32>(mix(checker, pixel.rgb, pixel.a), 1.0);
}
"#;

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let limits = device.limits();
        log::info!(
            "stage: window format {format:?}; device allows {} compute workgroups per dimension and {} storage buffers per stage",
            limits.max_compute_workgroups_per_dimension,
            limits.max_storage_buffers_per_shader_stage
        );
        let renderer = match vello::Renderer::new(device, vello::RendererOptions::default()) {
            Ok(renderer) => Some(renderer),
            Err(error) => {
                log::error!(
                    "stage: vello cannot run on this device, the stage stays blank: {error}"
                );
                None
            }
        };

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cuelight-stage-blit"),
            source: wgpu::ShaderSource::Wgsl(blit(format.is_srgb()).into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cuelight-stage-blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cuelight-stage-slice"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cuelight-stage-blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cuelight-stage-blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cuelight-stage-blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            gpu: Mutex::new(Gpu {
                presenter: Presenter::new(),
                renderer,
            }),
            blit,
            layout,
            sampler,
            queue: queue.clone(),
            uniform,
            bounds: [0.0, 0.0, 1.0, 1.0],
            srgb: format.is_srgb(),
            target: None,
            drawn: false,
        }
    }
}

impl Target {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        uniform: &wgpu::Buffer,
        srgb: bool,
        size: [u32; 2],
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cuelight-stage"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });
        let storage = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            }),
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cuelight-stage-blit"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&sampled),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Self {
            size,
            storage,
            bind_group,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuelight_core::Root;

    fn engine(layers: &str) -> Engine {
        let mut engine = Engine::new();
        engine
            .load_show(&format!(
                r#"{{"format": 1, "name": "t", "size": [200, 200], "layers": [{layers}]}}"#
            ))
            .unwrap();
        engine
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
    }

    #[test]
    fn an_upright_layer_gets_its_box() {
        let engine = engine(
            r##"{"name": "r", "type": "shape", "x": 10, "y": 20, "shape": {"rect": [0, 0, 40, 30]}, "fill": "#FFFFFF"}"##,
        );
        let [corners] = boxes(&engine, &[LayerPath::new(Root::Show, [0])])[..] else {
            panic!("one outline");
        };
        assert_eq!(
            corners,
            [[10.0, 20.0], [50.0, 20.0], [50.0, 50.0], [10.0, 50.0]]
        );
    }

    #[test]
    fn a_hidden_layer_has_no_outline() {
        let engine = engine(
            r##"{"name": "r", "type": "shape", "visible": false, "shape": {"rect": [0, 0, 40, 30]}, "fill": "#FFFFFF"}"##,
        );
        assert!(boxes(&engine, &[LayerPath::new(Root::Show, [0])]).is_empty());
    }

    #[test]
    fn a_turned_layer_gets_its_own_box_turned() {
        // A 40x20 rect turned a quarter round its corner at (100, 100):
        // the outline turns with it, rather than standing upright round it.
        let engine = engine(
            r##"{"name": "r", "type": "shape", "x": 100, "y": 100, "rotation": 90, "shape": {"rect": [0, 0, 40, 20]}, "fill": "#FFFFFF"}"##,
        );
        let [corners] = boxes(&engine, &[LayerPath::new(Root::Show, [0])])[..] else {
            panic!("one outline");
        };
        assert!(close(corners[0], [100.0, 100.0]), "{corners:?}");
        assert!(close(corners[1], [100.0, 140.0]), "{corners:?}");
        assert!(close(corners[2], [80.0, 140.0]), "{corners:?}");
        assert!(close(corners[3], [80.0, 100.0]), "{corners:?}");
    }

    #[test]
    fn handles_keep_their_size_at_any_zoom() {
        let upright = [[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]];
        let one = handles(upright, 1.0);
        assert_eq!(one.top, [20.0, 0.0]);
        assert!(close(one.knob, [20.0, -KNOB_GAP]), "{:?}", one.knob);
        let two = handles(upright, 2.0);
        assert!(close(two.knob, [20.0, -2.0 * KNOB_GAP]), "{:?}", two.knob);
        // Upside down, the knob is still over the box's own top edge.
        let flipped = [[40.0, 20.0], [0.0, 20.0], [0.0, 0.0], [40.0, 0.0]];
        let knob = handles(flipped, 1.0).knob;
        assert!(close(knob, [20.0, 20.0 + KNOB_GAP]), "{knob:?}");
    }

    /// What the stage's program sends, as a test sees it.
    #[derive(Debug, Clone, PartialEq)]
    enum Sent {
        Pick([f64; 2]),
        Press([f64; 2]),
        Grab(Grip, [f64; 2]),
        Drag([f64; 2]),
        Release,
    }

    fn stage(engine: Engine, selection: Vec<LayerPath>) -> Stage<Sent> {
        Stage {
            engine: Arc::new(Mutex::new(engine)),
            revision: 0,
            selection,
            guides: [None, None],
            on_pick: |at, _| Sent::Pick(at),
            on_press: Sent::Press,
            on_grab: Sent::Grab,
            on_drag: |at, _| Sent::Drag(at),
            on_release: || Sent::Release,
            whole: Default::default(),
        }
    }

    /// Hand `event` to the stage's program with the pointer at `at`, in
    /// a widget that draws the 200 x 200 show at 4x.
    fn send(stage: &Stage<Sent>, state: &mut State, event: Event, at: [f32; 2]) -> Option<Sent> {
        let bounds = Rectangle::new(iced::Point::ORIGIN, Size::new(800.0, 800.0));
        let cursor = mouse::Cursor::Available(iced::Point::new(at[0], at[1]));
        let action = shader::Program::update(stage, state, &event, bounds, cursor)?;
        action.into_inner().0
    }

    const PRESS: Event = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    const LET_GO: Event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));

    fn moved_to([x, y]: [f32; 2]) -> Event {
        Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(x, y),
        })
    }

    const RECT: &str = r##"{"name": "r", "type": "shape", "x": 10, "y": 20, "shape": {"rect": [0, 0, 40, 30]}, "fill": "#FFFFFF"}"##;

    #[test]
    fn a_click_picks_and_a_drag_on_the_selection_grabs_it() {
        let r = LayerPath::new(Root::Show, [0]);
        let stage = stage(engine(RECT), vec![r]);
        let mut state = State::default();
        // Inside the picked box: nothing until it lets go, then a pick.
        assert_eq!(send(&stage, &mut state, PRESS, [120.0, 120.0]), None);
        assert_eq!(
            send(&stage, &mut state, LET_GO, [120.0, 120.0]),
            Some(Sent::Pick([30.0, 30.0]))
        );
        // Moved past the slop: a grab from where it went down, then drags.
        assert_eq!(send(&stage, &mut state, PRESS, [120.0, 120.0]), None);
        assert_eq!(
            send(&stage, &mut state, moved_to([121.0, 120.0]), [121.0, 120.0]),
            None
        );
        assert_eq!(
            send(&stage, &mut state, moved_to([140.0, 120.0]), [140.0, 120.0]),
            Some(Sent::Grab(Grip::Move, [30.0, 30.0]))
        );
        assert_eq!(
            send(&stage, &mut state, moved_to([160.0, 120.0]), [160.0, 120.0]),
            Some(Sent::Drag([40.0, 30.0]))
        );
        assert_eq!(
            send(&stage, &mut state, LET_GO, [160.0, 120.0]),
            Some(Sent::Release)
        );
        // A corner scales; the knob over the top edge turns.
        let _ = send(&stage, &mut state, PRESS, [40.0, 80.0]);
        assert_eq!(
            send(&stage, &mut state, moved_to([20.0, 60.0]), [20.0, 60.0]),
            Some(Sent::Grab(Grip::Scale, [10.0, 20.0]))
        );
        let _ = send(&stage, &mut state, LET_GO, [20.0, 60.0]);
        let knob = [120.0, 80.0 - KNOB_GAP as f32];
        let _ = send(&stage, &mut state, PRESS, knob);
        assert!(matches!(
            send(&stage, &mut state, moved_to([160.0, 40.0]), [160.0, 40.0]),
            Some(Sent::Grab(Grip::Turn, _))
        ));
        let _ = send(&stage, &mut state, LET_GO, [160.0, 40.0]);
        // Off the selection a press picks at once.
        assert_eq!(
            send(&stage, &mut state, PRESS, [400.0, 400.0]),
            Some(Sent::Pick([100.0, 100.0]))
        );
    }

    #[test]
    fn a_ctrl_click_on_the_selection_is_still_the_shows_press() {
        let stage = stage(engine(RECT), vec![LayerPath::new(Root::Show, [0])]);
        let mut state = State::default();
        let _ = send(
            &stage,
            &mut state,
            Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::CTRL)),
            [0.0, 0.0],
        );
        assert_eq!(
            send(&stage, &mut state, PRESS, [120.0, 120.0]),
            Some(Sent::Press([30.0, 30.0]))
        );
    }

    #[test]
    fn with_two_picked_there_are_no_handles() {
        let engine = engine(&format!("{RECT}, {RECT}"));
        let fitted = Fitted::new(&engine, 800.0, 800.0).unwrap();
        let one = [LayerPath::new(Root::Show, [0])];
        let both = [
            LayerPath::new(Root::Show, [0]),
            LayerPath::new(Root::Show, [1]),
        ];
        assert_eq!(
            grip_at(&engine, &one, &fitted, [40.0, 80.0]),
            Some(Grip::Scale)
        );
        assert_eq!(
            grip_at(&engine, &both, &fitted, [40.0, 80.0]),
            Some(Grip::Move)
        );
        assert_eq!(grip_at(&engine, &one, &fitted, [600.0, 600.0]), None);
    }
}
