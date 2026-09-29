//! The window: an open bar with the transport, the stage beside what was
//! opened, and a status line.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use cuelight_core::{Influence, Layer, LayerKind, LayerPath, Property, TimelineOwner, Value};
use cuelight_editor_core::assets::{self, Asset, Kind};
use cuelight_editor_core::document::{Document, Pointer};
use cuelight_editor_core::inputs::{self, Inputs, Place};
use cuelight_editor_core::session::Instant;
use cuelight_editor_core::specimen::{self, Drawn, Sizing};
use cuelight_editor_core::syntax::{self, Token};
use cuelight_editor_core::tree::{self, Row};
use iced::keyboard;
use iced::widget::operation::{Animation, scroll_to, snap_to};
use iced::widget::pane_grid::{self, Axis, Configuration};
use iced::widget::scrollable::{AbsoluteOffset, Direction, RelativeOffset, Scrollbar};
use iced::widget::text::{Span, Wrapping};
use iced::widget::{
    Column, button, center, column, container, image, responsive, rich_text, row, scrollable,
    shader, slider, space, span, svg, text, text_input, toggler,
};
use iced::{ContentFit, Element, Fill, Font, Size, Subscription, Task, Theme};

use crate::dialog::{self, Picked};
use crate::stage::{Pick, Stage};
use cuelight_editor_core::opened::{self, Opened, Summary};
use cuelight_editor_core::session::Session;

/// What the command line asked for (desktop only).
#[cfg(not(target_arch = "wasm32"))]
#[derive(clap::Parser, Debug, Default, Clone, PartialEq)]
#[command(name = "cuelight-editor", about = "Editor for cuelight shows", version)]
pub struct Options {
    /// A show to open at once: a folder, a packed show or a show.json.
    pub show: Option<std::path::PathBuf>,
    /// Zoom the stage to this scale once the show is open.
    #[arg(long, value_name = "SCALE")]
    pub zoom: Option<f32>,
    /// Pick the layer under this canvas point once the show is open.
    #[arg(long, value_name = "X,Y", value_parser = parse_point)]
    pub pick: Option<[f64; 2]>,
    /// Show this asset in the inspector once the show is open, by its
    /// name in the library.
    #[arg(long, value_name = "NAME")]
    pub asset: Option<String>,
    /// Fire a trigger once the show is open; repeatable, in order.
    #[arg(long, value_name = "TRIGGER")]
    pub trigger: Vec<String>,
    /// Open no sound device.
    #[arg(long)]
    pub silent: bool,
    /// Write the window to this PNG once the show is drawn, then exit:
    /// how the editor is looked at without a screen, and how a change
    /// is checked in CI.
    #[arg(long, value_name = "OUT.png")]
    pub screenshot: Option<std::path::PathBuf>,
}

/// A canvas point as the command line spells it: `X,Y`.
#[cfg(not(target_arch = "wasm32"))]
fn parse_point(text: &str) -> Result<[f64; 2], String> {
    let point: Option<Vec<f64>> = text.split(',').map(|n| n.trim().parse().ok()).collect();
    match point.as_deref() {
        Some(&[x, y]) => Ok([x, y]),
        _ => Err(format!("not a point: {text:?}")),
    }
}

/// The command line's options, set by `main` before the app starts.
#[cfg(not(target_arch = "wasm32"))]
pub static OPTIONS: std::sync::OnceLock<Options> = std::sync::OnceLock::new();

pub struct App {
    session: Option<Session>,
    /// A screenshot asked for on the command line, and the frames left
    /// to draw before taking it: the stage needs a few to come up.
    #[cfg(not(target_arch = "wasm32"))]
    screenshot: Option<(std::path::PathBuf, u32)>,
    /// The sound device, opened for a show that has sounds (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    audio: Option<cuelight_audio::Output>,
    /// The browser's audio, made for a show that has sounds. Shared with
    /// the task that decodes them, which holds it only across each
    /// decode; a frame that finds it busy skips its sound.
    #[cfg(target_arch = "wasm32")]
    audio: Option<std::rc::Rc<std::cell::RefCell<cuelight_audio::WebAudio>>>,
    /// Where the open show came from, and what it holds.
    source: String,
    summary: Summary,
    /// The show's assets, and a thumbnail for each piece of artwork.
    library: Vec<Asset>,
    thumbs: Vec<Option<Thumb>>,
    /// How each font in the library looks, beside it.
    faces: Vec<Option<Faces>>,
    /// The outline fonts iced has been given, by name: a sample waits
    /// for its font, so it is not laid out in a fallback first.
    loaded: BTreeSet<String>,
    /// A sound playing from the library, outside the show's clock.
    preview: Option<Preview>,
    /// Which asset the library shows the facts of, and the inspector
    /// the preview of.
    selected: Option<usize>,
    /// The preview draws the artwork at its own size rather than fitted
    /// to the inspector.
    actual_size: bool,
    /// What the library area shows.
    tab: Tab,
    /// The tree's rows, as the open show has them.
    rows: Vec<Row>,
    /// The layers picked, in the order they were; the last is what the
    /// inspector shows.
    selection: Vec<LayerPath>,
    /// The inspector row unfolded to list every source of its value.
    expanded: Option<Property>,
    /// What the show can be told.
    inputs: Inputs,
    /// Variable fields being typed into, before they are submitted.
    edits: BTreeMap<String, String>,
    /// What each variable's field shows: the typed text while editing,
    /// the current value otherwise. Kept here because a field borrows it.
    fields: BTreeMap<String, String>,
    /// The last thing worth telling: an error, or what was just opened.
    status: String,
    /// A dialog is up; a second one is not opened over it.
    asking: bool,
    /// The three areas side by side, with splits to drag.
    panes: pane_grid::State<Pane>,
    /// How large the stage draws the show.
    zoom: Zoom,
    /// Physical pixels per logical one, which bounds how far the stage
    /// can zoom before its frame is more than vello draws.
    scale_factor: f32,
    /// The scale that fits the show into the stage area, as the last
    /// layout found it: what zooming in or out starts from while fitted.
    fitted: Cell<f32>,
    /// Whether the log below the stage is unfolded, or just its header.
    log_open: bool,
    /// Where the stage is scrolled to, as the last scroll left it; `None`
    /// while it is centred on the show, which a fresh open and a fit ask
    /// for. A zoom step scales it, so the point under the middle of the
    /// view stays there.
    scrolled: Option<AbsoluteOffset>,
    /// The show document as written, for the inspector to show a
    /// layer's own text.
    document: Option<Document>,
    /// Whether the system asks for a light or a dark theme: iced draws
    /// the window in the theme it picks for it, and the inspector's
    /// colours are taken from the same one.
    mode: iced::theme::Mode,
}

/// The stage's scroll pane, for the tasks that position it.
const STAGE: &str = "stage";

/// An area of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Library,
    Inputs,
    Stage,
    Inspector,
}

/// What the library area shows: the show's layers, or its assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Layers,
    Assets,
}

/// A thumbnail of a piece of artwork: an image's own pixels, or an
/// SVG's bytes for iced to draw.
enum Thumb {
    Image(image::Handle),
    Svg(svg::Handle),
}

/// A font as the library shows it: the sizes the show uses it at, the
/// sample line at the first, and the specimen at each.
struct Faces {
    sizings: Vec<Sizing>,
    sample: Face,
    /// Per sizing, each specimen row's code and characters with how
    /// they came out.
    specimens: Vec<Vec<(String, String, Face)>>,
}

/// A line of text in a show's font, ready to draw: the engine's pixels
/// for a bitmap or pixel font, the family for iced to draw an outline
/// font in, or nothing when the font has none of its characters.
#[derive(Clone)]
enum Face {
    Image {
        handle: image::Handle,
        width: u32,
        height: u32,
    },
    Outline {
        font: Font,
        /// The font's name in the show, to know when iced has it.
        name: String,
        size: f32,
    },
    Nothing,
}

/// A sound played once from the library: which asset it is, and since
/// when. Nothing is recorded and the show stays as it is.
#[derive(Debug, Clone)]
struct Preview {
    index: usize,
    sound: String,
    started: Instant,
    duration: f64,
}

/// The voice a preview plays under. The engine's own voice ids count up
/// from one and never reach this.
const PREVIEW_VOICE: u64 = u64::MAX;

/// The stage's magnification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// As large as the stage area allows, letterboxed like a player.
    Fit,
    /// One show pixel is this many logical pixels; 1.0 is 100%.
    Scale(f32),
}

impl Zoom {
    const MIN: f32 = 0.05;
    const MAX: f32 = 16.0;
    /// One step of the zoom buttons.
    const STEP: f32 = 1.25;
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenFile,
    #[cfg(not(target_arch = "wasm32"))]
    OpenFolder,
    Picked(Option<Picked>),
    #[cfg(not(target_arch = "wasm32"))]
    Dropped(std::path::PathBuf),
    /// A frame: the instant to move the show to.
    Tick(Instant),
    TogglePause,
    Restart,
    /// The playhead dragged to a time.
    Seek(f64),
    /// Forwards or back by this many seconds, paused.
    Step(f64),
    /// A key went down; the show's keys come first, then the editor's.
    KeyPressed(iced::keyboard::Key, iced::keyboard::Modifiers),
    /// A press on the stage, at a canvas point.
    Press([f64; 2]),
    /// A trigger fired from the panel.
    Fire(String),
    /// A variable's field being typed into.
    Edit(String, String),
    /// A variable set from its field (parsed) or its toggle.
    Set(String, String),
    Record(bool),
    /// The driver switched on or off.
    Drive(bool),
    Zoom(Zoom),
    /// Zoom in (above 1) or out (below 1) from the scale shown now.
    ZoomBy(f32),
    /// The stage's scroll pane moved, by hand or by a task.
    Scrolled(AbsoluteOffset),
    /// The library area switched to a tab.
    Tab(Tab),
    /// An asset picked in the library, or the pick cleared.
    Select(Option<usize>),
    /// An asset picked by its name, from the command line: the assets
    /// tab opens on it.
    #[cfg(not(target_arch = "wasm32"))]
    Reveal(String),
    /// The preview at the artwork's own size, or fitted.
    ActualSize(bool),
    /// An outline font of the show was given to iced to draw with.
    FontLoaded(String, bool),
    /// The play button of a sound in the library: play it once, or stop
    /// it while it plays.
    Preview(usize),
    /// A click on the stage at a canvas point: pick the layer there.
    Pick([f64; 2], Pick),
    /// A layer picked in the tree.
    Choose(LayerPath),
    /// The selection cleared.
    Deselect,
    /// An inspector row unfolded, or all folded.
    Expand(Option<Property>),
    /// The log below the stage folded to its header, or unfolded.
    ToggleLog,
    /// A split between two areas dragged.
    Resized(pane_grid::ResizeEvent),
    /// The system's light or dark preference, found or changed.
    Mode(iced::theme::Mode),
    /// The window's scale factor, found or changed.
    Rescaled(f32),
    /// The window as drawn, for `--screenshot`.
    #[cfg(not(target_arch = "wasm32"))]
    Shot(iced::window::Screenshot),
    /// The browser decoded the show's sounds: each name with its length,
    /// or why it did not decode.
    #[cfg(target_arch = "wasm32")]
    SoundsReady(Vec<(String, Result<f64, String>)>),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = Self {
            session: None,
            #[cfg(not(target_arch = "wasm32"))]
            screenshot: None,
            audio: None,
            source: String::new(),
            summary: Summary::default(),
            library: Vec::new(),
            thumbs: Vec::new(),
            faces: Vec::new(),
            loaded: BTreeSet::new(),
            preview: None,
            selected: None,
            actual_size: false,
            tab: Tab::Layers,
            rows: Vec::new(),
            selection: Vec::new(),
            expanded: None,
            inputs: Inputs::default(),
            edits: BTreeMap::new(),
            fields: BTreeMap::new(),
            status: String::new(),
            asking: false,
            panes: pane_grid::State::with_configuration(Configuration::Split {
                axis: Axis::Vertical,
                ratio: 0.24,
                a: Box::new(Configuration::Split {
                    axis: Axis::Horizontal,
                    ratio: 0.55,
                    a: Box::new(Configuration::Pane(Pane::Library)),
                    b: Box::new(Configuration::Pane(Pane::Inputs)),
                }),
                b: Box::new(Configuration::Split {
                    axis: Axis::Vertical,
                    ratio: 0.7,
                    a: Box::new(Configuration::Pane(Pane::Stage)),
                    b: Box::new(Configuration::Pane(Pane::Inspector)),
                }),
            }),
            zoom: Zoom::Fit,
            scrolled: None,
            document: None,
            mode: iced::theme::Mode::None,
            scale_factor: 1.0,
            fitted: Cell::new(1.0),
            log_open: true,
        };
        // A show on the command line opens at once (desktop only).
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut app = app;
            let options = OPTIONS.get().cloned().unwrap_or_default();
            app.screenshot = options.screenshot.map(|path| (path, 30));
            if let Some(scale) = options.zoom {
                app.zoom = Zoom::Scale(scale);
            }
            let open = match options.show {
                Some(path) => Task::done(Message::Dropped(path)),
                None => Task::none(),
            };
            let mut then = Task::none();
            for trigger in options.trigger {
                then = then.chain(Task::done(Message::Fire(trigger)));
            }
            if let Some(point) = options.pick {
                then = then.chain(Task::done(Message::Pick(point, Pick::default())));
            }
            if let Some(name) = options.asset {
                then = then.chain(Task::done(Message::Reveal(name)));
            }
            (app, Task::batch([system_mode(), open.chain(then)]))
        }
        // A page asked to open a show (`?show=<url>`) fetches it.
        #[cfg(target_arch = "wasm32")]
        {
            let mut app = app;
            app.asking = true;
            (
                app,
                Task::batch([
                    system_mode(),
                    Task::perform(dialog::fetch_show_from_query(), Message::Picked),
                ]),
            )
        }
    }

    pub fn title(&self) -> String {
        match &self.session {
            Some(_) => format!("{} - cuelight editor", self.summary.name),
            None => "cuelight editor".to_owned(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // A browser keeps a page silent until someone acts on it; any
        // message but a frame is such an act.
        #[cfg(target_arch = "wasm32")]
        if !matches!(message, Message::Tick(_))
            && let Some(audio) = &self.audio
            && let Ok(audio) = audio.try_borrow()
            && !audio.running()
        {
            audio.resume();
        }
        let task = self.handle(message);
        self.refresh_fields();
        task
    }

    /// What the variable fields show now.
    fn refresh_fields(&mut self) {
        let Some(session) = &self.session else {
            self.fields.clear();
            return;
        };
        for (name, initial) in &self.inputs.variables {
            let shown = match self.edits.get(name) {
                Some(typed) => typed.clone(),
                None => inputs::show_value(&session.value(name).unwrap_or_else(|| initial.clone())),
            };
            self.fields.insert(name.clone(), shown);
        }
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenFile => {
                if self.asking {
                    return Task::none();
                }
                self.asking = true;
                Task::perform(dialog::pick_file(), Message::Picked)
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::OpenFolder => {
                if self.asking {
                    return Task::none();
                }
                self.asking = true;
                Task::perform(dialog::pick_folder(), Message::Picked)
            }
            Message::Picked(picked) => {
                self.asking = false;
                match picked {
                    #[cfg(not(target_arch = "wasm32"))]
                    Some(Picked::Path(path)) => self.open(Opened::from_path(&path)),
                    #[cfg(target_arch = "wasm32")]
                    Some(Picked::File { name, bytes }) => {
                        self.open(Opened::from_bytes(&name, &bytes))
                    }
                    None => Task::none(),
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::Dropped(path) => self.open(Opened::from_path(&path)),
            #[cfg(target_arch = "wasm32")]
            Message::SoundsReady(sounds) => {
                for (name, result) in sounds {
                    if let Err(error) = result {
                        self.status = format!("sound {name} did not decode: {error}");
                        log::warn!("{}", self.status);
                    }
                }
                Task::none()
            }
            Message::Tick(now) => {
                if let Some(session) = &mut self.session
                    && !session.paused
                {
                    session.tick(now);
                    self.hear();
                } else if self.preview.is_some() {
                    // Paused, but a previewed sound plays on its own clock.
                    self.hear();
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Some((_, frames)) = &mut self.screenshot {
                    *frames = frames.saturating_sub(1);
                    if *frames == 0 {
                        return iced::window::latest()
                            .and_then(iced::window::screenshot)
                            .map(Message::Shot);
                    }
                }
                Task::none()
            }
            Message::Seek(to) => {
                if let Some(session) = &mut self.session {
                    // Dragging the playhead pauses; sound stays silent
                    // while scrubbing.
                    session.paused = true;
                    session.seek(to, Instant::now());
                    self.hush();
                }
                Task::none()
            }
            Message::Step(dt) => {
                if let Some(session) = &mut self.session {
                    session.step(dt, Instant::now());
                    self.hush();
                }
                Task::none()
            }
            Message::KeyPressed(key, modifiers) => {
                let name = key_name(&key);
                // The show's keys first; with Ctrl held the editor's own
                // shortcuts are reached whatever the show maps.
                if !modifiers.control()
                    && let Some(session) = &mut self.session
                    && self.inputs.keys.contains_key(name.as_str())
                {
                    session.key(&name);
                    return Task::none();
                }
                match name.as_str() {
                    "Escape" => self.update(Message::Deselect),
                    " " => self.update(Message::TogglePause),
                    "r" => self.update(Message::Restart),
                    "f" | "F" => self.update(Message::Zoom(Zoom::Fit)),
                    "0" if modifiers.control() => self.update(Message::Zoom(Zoom::Fit)),
                    "1" if modifiers.control() => self.update(Message::Zoom(Zoom::Scale(1.0))),
                    "=" | "+" if modifiers.control() => self.update(Message::ZoomBy(Zoom::STEP)),
                    "-" if modifiers.control() => self.update(Message::ZoomBy(1.0 / Zoom::STEP)),
                    "," | "<" => self.update(Message::Step(if modifiers.shift() {
                        -1.0
                    } else {
                        -1.0 / 60.0
                    })),
                    "." | ">" => self.update(Message::Step(if modifiers.shift() {
                        1.0
                    } else {
                        1.0 / 60.0
                    })),
                    _ => Task::none(),
                }
            }
            Message::Press(at) => {
                if let Some(session) = &mut self.session {
                    session.press(at);
                }
                Task::none()
            }
            Message::Fire(trigger) => {
                if let Some(session) = &mut self.session {
                    session.fire(&trigger);
                }
                Task::none()
            }
            Message::Edit(name, text) => {
                self.edits.insert(name, text);
                Task::none()
            }
            Message::Set(name, text) => {
                self.edits.remove(&name);
                if let Some(session) = &mut self.session {
                    session.set(&name, inputs::parse_value(&text));
                }
                Task::none()
            }
            Message::Record(on) => {
                if let Some(session) = &mut self.session {
                    session.recording = on;
                }
                Task::none()
            }
            Message::ToggleLog => {
                self.log_open = !self.log_open;
                Task::none()
            }
            Message::Drive(on) => {
                if let Some(session) = &mut self.session {
                    session.set_driving(on, Instant::now());
                    self.hush();
                }
                Task::none()
            }
            Message::TogglePause => {
                if let Some(session) = &mut self.session {
                    session.toggle_pause(Instant::now());
                    if session.paused {
                        self.hush();
                    }
                }
                Task::none()
            }
            Message::Restart => {
                if let Some(session) = &mut self.session {
                    session.restart(Instant::now());
                }
                Task::none()
            }
            Message::Zoom(zoom) => {
                let before = self.scale();
                self.zoom = match zoom {
                    Zoom::Fit => Zoom::Fit,
                    Zoom::Scale(scale) => self.zoom_to(scale),
                };
                match zoom {
                    Zoom::Fit => self.centre_stage(),
                    Zoom::Scale(_) => self.keep_middle(before),
                }
            }
            Message::ZoomBy(factor) => {
                let before = self.scale();
                self.zoom = self.zoom_to(before * factor);
                self.keep_middle(before)
            }
            Message::Scrolled(offset) => {
                self.scrolled = Some(offset);
                Task::none()
            }
            Message::Rescaled(factor) => {
                self.scale_factor = factor;
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::Shot(shot) => {
                let Some((path, _)) = self.screenshot.take() else {
                    return Task::none();
                };
                match write_png(&path, &shot) {
                    Ok(()) => log::info!("screenshot: {}", path.display()),
                    Err(error) => log::error!("screenshot {}: {error}", path.display()),
                }
                iced::exit()
            }
            Message::Tab(tab) => {
                self.tab = tab;
                Task::none()
            }
            Message::Select(index) => {
                self.selected = index.filter(|i| *i < self.library.len());
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::Reveal(name) => {
                match self.library.iter().position(|a| a.name == name) {
                    Some(index) => {
                        self.tab = Tab::Assets;
                        self.selected = Some(index);
                    }
                    None => {
                        self.status = format!("no asset {name:?} in this show");
                        log::warn!("{}", self.status);
                    }
                }
                Task::none()
            }
            Message::ActualSize(on) => {
                self.actual_size = on;
                Task::none()
            }
            Message::FontLoaded(name, loaded) => {
                if loaded {
                    self.loaded.insert(name);
                }
                Task::none()
            }
            Message::Preview(index) => {
                match &self.preview {
                    Some(preview) if preview.index == index => self.preview = None,
                    _ => {
                        let duration = self.library.get(index).and_then(|asset| {
                            let session = self.session.as_ref()?;
                            let engine = session.engine.lock().expect("the engine is not poisoned");
                            Some((asset.name.clone(), engine.sound_duration(&asset.name)?))
                        });
                        self.preview = duration.map(|(sound, duration)| Preview {
                            index,
                            sound,
                            started: Instant::now(),
                            duration,
                        });
                    }
                }
                self.hear();
                Task::none()
            }
            Message::Pick(point, pick) => {
                let Some(session) = &self.session else {
                    return Task::none();
                };
                let under = session
                    .engine
                    .lock()
                    .expect("the engine is not poisoned")
                    .layers_at(point);
                self.pick(under, pick);
                // A layer picked is what the inspector shows now.
                self.selected = None;
                Task::none()
            }
            Message::Choose(path) => {
                // Also from the inspector's list of an asset's uses: the
                // layer is then shown where the tree has it.
                self.tab = Tab::Layers;
                self.selection = vec![path];
                self.selected = None;
                self.expanded = None;
                Task::none()
            }
            Message::Deselect => {
                self.selection.clear();
                self.selected = None;
                self.expanded = None;
                Task::none()
            }
            Message::Expand(property) => {
                self.expanded = property;
                Task::none()
            }
            Message::Mode(mode) => {
                self.mode = mode;
                Task::none()
            }
            Message::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
                Task::none()
            }
        }
    }

    /// What a click on the stage does to the selection, given the layers
    /// under it, topmost first: a plain click takes the topmost, Alt the
    /// next one down from the one picked last, Shift adds or removes
    /// rather than replaces; a click on nothing clears.
    fn pick(&mut self, under: Vec<LayerPath>, pick: Pick) {
        self.expanded = None;
        let Some(first) = under.first() else {
            if !pick.shift {
                self.selection.clear();
            }
            return;
        };
        let chosen = if pick.alt {
            let last = self.selection.last();
            let at = last.and_then(|last| under.iter().position(|p| p == last));
            at.map_or(first, |i| &under[(i + 1) % under.len()])
        } else {
            first
        };
        if pick.shift {
            match self.selection.iter().position(|p| p == chosen) {
                Some(i) => {
                    self.selection.remove(i);
                }
                None => self.selection.push(chosen.clone()),
            }
        } else {
            self.selection = vec![chosen.clone()];
        }
    }

    /// The scale the stage draws at now.
    fn scale(&self) -> f32 {
        match self.zoom {
            Zoom::Fit => self.fitted.get(),
            Zoom::Scale(scale) => scale,
        }
    }

    /// A zoom to `scale`, kept between the smallest and the largest the
    /// stage can draw: vello stops past a frame of about 4096 x 4096
    /// physical pixels, whatever the show's size.
    fn zoom_to(&self, scale: f32) -> Zoom {
        Zoom::Scale(scale.clamp(Zoom::MIN, self.max_zoom()))
    }

    /// Put the show in the middle of the stage: what a fresh open and a
    /// fit do.
    fn centre_stage(&mut self) -> Task<Message> {
        self.scrolled = None;
        snap_to(STAGE, RelativeOffset { x: 0.5, y: 0.5 }, Animation::Instant)
    }

    /// After a zoom from the scale `before`, keep the canvas point that was
    /// under the middle of the view there. The room round the show is half
    /// the view on every side, so that point is the scroll offset over the
    /// scale, and the new offset is the old one scaled.
    fn keep_middle(&mut self, before: f32) -> Task<Message> {
        let Some(offset) = self.scrolled else {
            return self.centre_stage();
        };
        let ratio = self.scale() / before.max(f32::EPSILON);
        let to = AbsoluteOffset {
            x: offset.x * ratio,
            y: offset.y * ratio,
        };
        self.scrolled = Some(to);
        scroll_to(STAGE, to, Animation::Instant)
    }

    /// The largest zoom whose frame vello still draws.
    fn max_zoom(&self) -> f32 {
        let [w, h] = self.summary.size.map(|n| n.max(1) as f32);
        let frame = |scale: f32| {
            [
                (w * scale * self.scale_factor).round() as u32,
                (h * scale * self.scale_factor).round() as u32,
            ]
        };
        let mut scale = Zoom::MAX;
        while scale > Zoom::MIN && !crate::stage::drawable(frame(scale)) {
            scale /= 1.02;
        }
        scale.max(Zoom::MIN)
    }

    fn open(&mut self, result: Result<Opened, opened::OpenError>) -> Task<Message> {
        match result {
            Ok(opened) => {
                self.status = format!("opened {}", opened.source);
                log::info!("{}", self.status);
                let Opened {
                    source,
                    engine,
                    summary,
                    driver,
                    sound_files,
                    sounds,
                    library,
                    files,
                    document,
                    ..
                } = opened;
                self.source = source;
                self.summary = summary;
                self.thumbs = thumbs(&engine, &library);
                self.faces = faces(&engine, &files, &library);
                self.loaded.clear();
                let fonts = load_fonts(&engine, &library);
                self.library = library;
                self.preview = None;
                self.selected = None;
                self.rows = engine.show().map(tree::rows).unwrap_or_default();
                self.selection.clear();
                self.expanded = None;
                self.inputs = engine.show().map(Inputs::of).unwrap_or_default();
                self.edits.clear();
                let task = self.listen(&sounds, sound_files);
                let mut session = Session::new(engine, driver);
                // The log opens with what the load had to say and what
                // the audit makes of the document as written.
                session.files = files.keys().cloned().collect();
                session
                    .log
                    .extend(cuelight_editor_core::log::load(&self.summary.problems));
                session.audit(&document.text());
                self.document = Some(document);
                self.session = Some(session);
                // The window's scale factor bounds the zoom; ask once a
                // window is there to ask.
                let rescaled = iced::window::latest()
                    .and_then(iced::window::scale_factor)
                    .map(Message::Rescaled);
                let centred = self.centre_stage();
                Task::batch([task, fonts, rescaled, centred])
            }
            Err(error) => {
                self.status = format!("could not open: {error}");
                log::warn!("{}", self.status);
                Task::none()
            }
        }
    }

    /// Give the sound device the show's sounds, opening it for the first
    /// show that has any.
    #[cfg(not(target_arch = "wasm32"))]
    fn listen(
        &mut self,
        sounds: &[(String, std::sync::Arc<cuelight_editor_core::opened::Sound>)],
        _files: Vec<cuelight_editor_core::opened::SoundFile>,
    ) -> Task<Message> {
        if sounds.is_empty() || OPTIONS.get().is_some_and(|o| o.silent) {
            return Task::none();
        }
        if self.audio.is_none() {
            self.audio = cuelight_audio::Output::open()
                .map_err(|error| log::warn!("no sound: {error}"))
                .ok();
        }
        if let Some(audio) = &self.audio {
            for (name, sound) in sounds {
                audio.set_sound(name, sound.clone());
            }
        }
        Task::none()
    }

    /// Have the browser decode the show's sounds, one after the other,
    /// making its audio for the first show that has any. The lengths the
    /// engine needs are registered already, from decoding them here.
    ///
    /// The audio is held across each decode on purpose: the page is
    /// single-threaded, so nothing waits on the borrow, and a frame that
    /// finds it busy skips its sound rather than block.
    #[cfg(target_arch = "wasm32")]
    #[allow(clippy::await_holding_refcell_ref)]
    fn listen(
        &mut self,
        _sounds: &[(String, std::sync::Arc<cuelight_editor_core::opened::Sound>)],
        files: Vec<cuelight_editor_core::opened::SoundFile>,
    ) -> Task<Message> {
        use std::cell::RefCell;
        use std::rc::Rc;
        if files.is_empty() {
            return Task::none();
        }
        if self.audio.is_none() {
            self.audio = cuelight_audio::WebAudio::new()
                .map_err(|error| log::warn!("no sound: {error:?}"))
                .ok()
                .map(|audio| Rc::new(RefCell::new(audio)));
        }
        let Some(audio) = self.audio.clone() else {
            return Task::none();
        };
        Task::perform(
            async move {
                let mut done = Vec::new();
                for file in files {
                    let result = audio.borrow_mut().decode(&file.name, &file.bytes).await;
                    done.push((file.name, result.map_err(|e| format!("{e:?}"))));
                }
                done
            },
            Message::SoundsReady,
        )
    }

    /// The voices to play now: the show's while it plays (with `show`),
    /// and the sound previewed from the library, which keeps its own
    /// clock and is over once it has run its length.
    fn voices(&mut self, show: bool) -> Vec<cuelight_core::Voice> {
        let mut voices = Vec::new();
        if show
            && let Some(session) = &self.session
            && !session.paused
        {
            let engine = session.engine.lock().expect("the engine is not poisoned");
            match engine.voices() {
                Ok(heard) => voices = heard,
                Err(error) => log::warn!("voices: {error}"),
            }
        }
        if let Some(preview) = &self.preview {
            let position = preview.started.elapsed().as_secs_f64();
            if position >= preview.duration {
                self.preview = None;
            } else {
                voices.push(cuelight_core::Voice {
                    id: PREVIEW_VOICE,
                    layer: "library".to_owned(),
                    sound: preview.sound.clone(),
                    position,
                    gain: 1.0,
                    looping: false,
                    bus: None,
                });
            }
        }
        voices
    }

    /// Play what the show sounds like now, and the preview if one plays.
    fn hear(&mut self) {
        let voices = self.voices(true);
        self.play(&voices);
    }

    /// Silence the show, for a scrub or a pause; a preview plays on.
    fn hush(&mut self) {
        let voices = self.voices(false);
        self.play(&voices);
    }

    /// Hand `voices` to the sound device or the browser's audio.
    fn play(&mut self, voices: &[cuelight_core::Voice]) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(audio) = &self.audio {
            audio.apply(voices);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(audio) = &self.audio
            && let Ok(mut audio) = audio.try_borrow_mut()
        {
            audio.apply(voices);
        }
    }

    /// Whether a sound can be heard at all: there is a device or a
    /// browser to play it, and `--silent` was not asked.
    fn can_play(&self) -> bool {
        self.audio.is_some()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keys = keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed { key, modifiers, .. } => {
                Some(Message::KeyPressed(key, modifiers))
            }
            _ => None,
        });
        let rescaled = iced::window::events().filter_map(|(_, event)| match event {
            iced::window::Event::Rescaled(factor) => Some(Message::Rescaled(factor)),
            _ => None,
        });
        let mut subscriptions = vec![
            keys,
            rescaled,
            iced::system::theme_changes().map(Message::Mode),
        ];
        #[cfg(not(target_arch = "wasm32"))]
        let waiting_to_shoot = self.session.is_some() && self.screenshot.is_some();
        #[cfg(target_arch = "wasm32")]
        let waiting_to_shoot = false;
        // Frames while the show plays, and while a previewed sound does:
        // its end is noticed on a frame.
        if self.session.as_ref().is_some_and(|s| !s.paused)
            || waiting_to_shoot
            || self.preview.is_some()
        {
            subscriptions.push(iced::window::frames().map(Message::Tick));
        }
        #[cfg(not(target_arch = "wasm32"))]
        subscriptions.push(iced::event::listen_with(
            |event, _status, _window| match event {
                iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                    Some(Message::Dropped(path))
                }
                _ => None,
            },
        ));
        // A browser does not tell the window about drops: the page listens.
        #[cfg(target_arch = "wasm32")]
        subscriptions
            .push(Subscription::run(dialog::drops).map(|picked| Message::Picked(Some(picked))));
        Subscription::batch(subscriptions)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut bar = row![
            button("Open file...").on_press_maybe((!self.asking).then_some(Message::OpenFile))
        ]
        .spacing(8);
        #[cfg(not(target_arch = "wasm32"))]
        {
            bar = bar.push(
                button("Open folder...")
                    .on_press_maybe((!self.asking).then_some(Message::OpenFolder)),
            );
        }
        if let Some(session) = &self.session {
            // The playhead covers one pass of the driver, or as far as
            // the show has played, whichever is longer.
            let end = session
                .pass_length()
                .unwrap_or(60.0)
                .max(session.time)
                .max(1.0);
            bar = bar
                .push(space::horizontal().width(16))
                .push(button("|<").on_press(Message::Restart))
                .push(button("<").on_press(Message::Step(-1.0 / 60.0)))
                .push(
                    button(if session.paused { "Play" } else { "Pause" })
                        .on_press(Message::TogglePause),
                )
                .push(button(">").on_press(Message::Step(1.0 / 60.0)))
                .push(
                    slider(0.0..=end, session.time, Message::Seek)
                        .step(1.0 / 60.0)
                        .width(Fill),
                )
                .push(text(format!("{:7.2} / {end:.0} s", session.time)).size(14))
                .push(space::horizontal().width(16));
            if session.has_driver() {
                bar = bar.push(
                    toggler(session.driving)
                        .label("Driver")
                        .on_toggle(Message::Drive)
                        .size(16),
                );
            }
            bar = bar
                .push(space::horizontal().width(16))
                .push(text(&self.source).size(14));
        }

        let body: Element<'_, Message> = match &self.session {
            None => center(
                text(if cfg!(target_arch = "wasm32") {
                    "Open a packed show (.cuelight), or drop one on this page."
                } else {
                    "Open a show folder, a packed show (.cuelight) or a show.json, or drop one on this window."
                })
                .size(18),
            )
            .into(),
            Some(session) => iced::widget::pane_grid(&self.panes, move |_, pane, _| {
                pane_grid::Content::new(match pane {
                    Pane::Inputs => Element::from(
                        scrollable(self.inputs_panel(session))
                            .width(Fill)
                            .height(Fill),
                    ),
                    Pane::Stage => column![
                        responsive(move |size| self.stage(session, size)),
                        self.log_panel(session),
                    ]
                    .into(),
                    Pane::Library => self.library_panel(session),
                    // The preview of an asset fits the pane, so the pane
                    // says how large it is.
                    Pane::Inspector => responsive(move |size| {
                        scrollable(self.inspector_panel(session, size))
                            .width(Fill)
                            .height(Fill)
                    })
                    .into(),
                })
            })
            .on_resize(8, Message::Resized)
            .spacing(4)
            .min_size(120)
            .width(Fill)
            .height(Fill)
            .into(),
        };

        let status = container(text(&self.status).size(13))
            .padding([4, 8])
            .width(Fill);

        column![container(bar).padding(8).width(Fill), body, status].into()
    }
}

impl App {
    /// The stage area: a zoom bar over the show drawn at its scale, in a
    /// scroll pane with room round it of half the view on every side, so
    /// the show can be scrolled until its edge sits in the middle of the
    /// view, the way drawing tools have it.
    fn stage<'a>(&'a self, session: &'a Session, size: Size) -> Element<'a, Message> {
        const BAR: f32 = 36.0;
        const MARGIN: f32 = 8.0;
        let [show_w, show_h] = self.summary.size.map(|n| n.max(1) as f32);
        let room = Size::new(
            (size.width - 2.0 * MARGIN).max(1.0),
            (size.height - BAR - 2.0 * MARGIN).max(1.0),
        );
        let fit = (room.width / show_w).min(room.height / show_h);
        self.fitted.set(fit);
        let scale = match self.zoom {
            Zoom::Fit => fit,
            Zoom::Scale(scale) => scale,
        };
        let (w, h) = ((show_w * scale).round(), (show_h * scale).round());
        // The room round the show: half the view on every side, so the
        // pane's bars stand for show plus room and its own background
        // shows round the show; the scrollbars float over room, never
        // over the show's far edge.
        let around = iced::Padding {
            top: (room.height / 2.0).round(),
            bottom: (room.height / 2.0).round(),
            left: (room.width / 2.0).round(),
            right: (room.width / 2.0).round(),
        };

        let zoom_button = |label: &'a str, zoom: Zoom| {
            let mut b = button(text(label).size(13)).on_press(Message::Zoom(zoom));
            if self.zoom == zoom {
                b = b.style(button::secondary);
            }
            b
        };
        let bar = row![
            zoom_button("Fit", Zoom::Fit),
            zoom_button("100%", Zoom::Scale(1.0)),
            button(text("-").size(13)).on_press(Message::ZoomBy(1.0 / Zoom::STEP)),
            button(text("+").size(13)).on_press(Message::ZoomBy(Zoom::STEP)),
            text(format!("{:.0}%", scale * 100.0)).size(13),
        ]
        .spacing(6)
        .align_y(iced::Center);

        let stage = shader(Stage {
            engine: session.engine.clone(),
            revision: session.revision,
            selection: self.selection.clone(),
            on_pick: Message::Pick,
            on_press: Message::Press,
        })
        .width(w)
        .height(h);
        // The pane fills the room, with the margin outside it, and is
        // positioned by the tasks that centre it and keep the middle on
        // a zoom.
        let scrolled = scrollable(container(stage).padding(around))
            .id(STAGE)
            .width(Fill)
            .height(Fill)
            .on_scroll(|scroll| Message::Scrolled(scroll.viewport.absolute_offset()))
            .direction(Direction::Both {
                vertical: Scrollbar::default(),
                horizontal: Scrollbar::default(),
            })
            // The corner where the two scrollbars meet is the window's,
            // not the show's.
            .style(|theme, status| {
                let mut style = scrollable::default(theme, status);
                style.gap = Some(theme.palette().background.weak.color.into());
                style
            });
        column![
            container(bar).padding([4, 8]).height(BAR),
            container(scrolled).padding(MARGIN).width(Fill).height(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// The show's inputs: triggers as buttons, variables as fields, the
    /// show's own values as readouts.
    fn inputs_panel<'a>(&'a self, session: &'a Session) -> Column<'a, Message> {
        let mut panel = Column::new().spacing(6).padding(12);
        panel = panel.push(
            toggler(session.recording)
                .label("Record what I fire")
                .on_toggle(Message::Record)
                .size(16),
        );
        // Triggers by where they are heard: the ones that open a scene,
        // the ones heard anywhere, then each scene's own, dimmed while
        // another scene is up.
        let active = session.active_scene();
        let mut opens = Vec::new();
        let mut anywhere = Vec::new();
        let mut by_scene: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for trigger in &self.inputs.triggers {
            match self.inputs.places.get(trigger) {
                Some(Place::Opens(_)) => opens.push(trigger.as_str()),
                Some(Place::Scene(scene)) => {
                    by_scene.entry(scene.as_str()).or_default().push(trigger)
                }
                _ => anywhere.push(trigger.as_str()),
            }
        }
        if !opens.is_empty() {
            panel = self.triggers(panel, "OPENS A SCENE".to_owned(), &opens, true);
        }
        if !anywhere.is_empty() {
            panel = self.triggers(panel, "ANYWHERE".to_owned(), &anywhere, true);
        }
        for (scene, triggers) in &by_scene {
            let live = active.as_deref() == Some(*scene);
            panel = self.triggers(panel, format!("IN {scene}"), triggers, live);
        }
        if !self.inputs.variables.is_empty() {
            panel = panel.push(text("VARIABLES").size(12));
            for (name, initial) in &self.inputs.variables {
                let current = session.value(name).unwrap_or_else(|| initial.clone());
                let control: Element<'a, Message> = match current {
                    cuelight_core::Value::Bool(on) => toggler(on)
                        .on_toggle(move |on| Message::Set(name.clone(), on.to_string()))
                        .size(16)
                        .into(),
                    _ => {
                        let shown: &'a str =
                            self.fields.get(name).map(String::as_str).unwrap_or("");
                        text_input("", shown)
                            .on_input(move |t| Message::Edit(name.clone(), t))
                            .on_submit(Message::Set(name.clone(), shown.to_owned()))
                            .size(14)
                            .width(110)
                            .into()
                    }
                };
                panel = panel.push(row![text(name).size(14).width(Fill), control].spacing(8));
            }
        }
        if !self.inputs.values.is_empty() {
            panel = panel.push(text("VALUES").size(12));
            for name in &self.inputs.values {
                let shown = session
                    .value(name)
                    .map(|v| inputs::show_value(&v))
                    .unwrap_or_default();
                panel = panel
                    .push(row![text(name).size(14).width(Fill), text(shown).size(14)].spacing(8));
            }
        }
        panel
    }
}

impl App {
    /// The log under the stage: a header saying how many lines, which
    /// folds it to itself; unfolded, the lines follow the newest.
    fn log_panel<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        const HEIGHT: f32 = 160.0;
        let count = session.log.len();
        let header = row![
            text("LOG").size(12),
            text(format!("{count} line(s)")).size(12),
            space::horizontal(),
            button(text(if self.log_open { "fold" } else { "unfold" }).size(12))
                .on_press(Message::ToggleLog)
                .style(button::text),
        ]
        .spacing(12)
        .align_y(iced::Center);
        let mut panel = column![container(header).padding([0, 8]).width(Fill)].spacing(4);
        if self.log_open {
            let mut lines = Column::new().spacing(1).padding([0, 8]);
            for line in session.log.lines() {
                lines = lines.push(
                    text(line.render())
                        .size(12)
                        .font(iced::Font::new("DM Mono"))
                        .wrapping(text::Wrapping::None),
                );
            }
            panel = panel.push(
                scrollable(lines)
                    .direction(Direction::Both {
                        vertical: Scrollbar::default(),
                        horizontal: Scrollbar::default(),
                    })
                    .anchor_bottom()
                    .width(Fill)
                    .height(HEIGHT),
            );
        }
        container(panel).padding([4, 0]).width(Fill).into()
    }

    /// The library area: a tab row over the show's layers or its assets.
    fn library_panel<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let tab = |label: &'a str, tab: Tab| {
            let mut b = button(text(label).size(13)).on_press(Message::Tab(tab));
            if self.tab == tab {
                b = b.style(button::secondary);
            }
            b
        };
        let tabs = row![tab("Layers", Tab::Layers), tab("Assets", Tab::Assets)].spacing(6);
        let body = match self.tab {
            Tab::Layers => self.tree_panel(session),
            Tab::Assets => self.assets_panel(),
        };
        column![
            container(tabs).padding([4, 8]),
            scrollable(body).width(Fill).height(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// The layers as a tree: the show's own, then each scene's, groups'
    /// children indented under them; the picked ones marked, the active
    /// scene starred.
    fn tree_panel<'a>(&'a self, session: &'a Session) -> Column<'a, Message> {
        let mut panel = Column::new().spacing(2).padding(12);
        let active = session.active_scene();
        for row_ in &self.rows {
            match row_ {
                Row::Root { root, name } => {
                    let heading = match root {
                        cuelight_core::Root::Show => "SHOW".to_owned(),
                        cuelight_core::Root::Scene(_) if active.as_deref() == Some(name) => {
                            format!("SCENE {name} *")
                        }
                        cuelight_core::Root::Scene(_) => format!("SCENE {name}"),
                    };
                    panel = panel.push(container(text(heading).size(12)).padding([6, 0]));
                }
                Row::Layer {
                    path,
                    name,
                    kind,
                    depth,
                } => {
                    let line = row![
                        space::horizontal().width(*depth as f32 * 14.0),
                        text(*kind).size(11).width(44),
                        text(name).size(14),
                    ]
                    .spacing(6)
                    .align_y(iced::Center);
                    let mut b = button(line)
                        .on_press(Message::Choose(path.clone()))
                        .width(Fill)
                        .padding([2, 6])
                        .style(button::text);
                    if self.selection.contains(path) {
                        b = b.style(button::secondary);
                    }
                    panel = panel.push(b);
                }
            }
        }
        panel
    }

    /// The inspector: the picked layer's properties with their live
    /// values and where each comes from, its bindings and timelines, and
    /// its JSON; the show's own facts while nothing is picked. With the
    /// assets showing, the picked asset.
    fn inspector_panel<'a>(&'a self, session: &'a Session, size: Size) -> Column<'a, Message> {
        if self.tab == Tab::Assets
            && let Some(i) = self.selected
        {
            return self.asset_panel(session, i, size);
        }
        let Some(path) = self.selection.last() else {
            return summary(&self.summary);
        };
        let engine = session.engine.lock().expect("the engine is not poisoned");
        let Some(show) = engine.show() else {
            return summary(&self.summary);
        };
        let Some(layer) = tree::layer(show, path) else {
            return Column::new().push(text("the picked layer is gone").size(14));
        };
        let mut panel = Column::new().spacing(4).padding(12);
        panel = panel.push(text(layer.name.clone()).size(16));
        panel = panel.push(
            text(format!(
                "{}, {}",
                tree::kind_name(&layer.kind),
                tree::describe(show, path)
            ))
            .size(12),
        );
        if let LayerKind::Part { id, pivot } = &layer.kind {
            // An element of the artwork above it, moved in the artwork's
            // own coordinates around its pivot.
            let around = match pivot {
                Some([x, y]) => format!("pivot {x}, {y}"),
                None => "pivot at the centre of its bounds".to_owned(),
            };
            panel = panel.push(text(format!("element {id:?} of the artwork, {around}")).size(12));
        }
        if self.selection.len() > 1 {
            panel = panel.push(text(format!("{} picked", self.selection.len())).size(12));
        }

        // Every property the layer has, its value now, and its sources.
        let live: Vec<(Property, Value)> = engine
            .values()
            .unwrap_or_default()
            .into_iter()
            .filter(|v| v.layer == *path)
            .map(|v| (v.property, v.value))
            .collect();
        let mut owned = Vec::new();
        let mut heading = "";
        for property in tree::PROPERTIES {
            let sources = engine.explain(path, property);
            if sources.is_empty() {
                continue;
            }
            let group = match property {
                Property::X
                | Property::Y
                | Property::Rotation
                | Property::Scale
                | Property::ScaleX
                | Property::ScaleY => "PLACEMENT",
                _ => "APPEARANCE",
            };
            if group != heading {
                heading = group;
                panel = panel.push(container(text(group).size(12)).padding([6, 0]));
            }
            let value = live
                .iter()
                .find(|(p, _)| *p == property)
                .map(|(_, v)| inputs::show_value(v))
                .or_else(|| sources.iter().find_map(influence_value))
                .unwrap_or_default();
            let badge = sources.first().map(winner).unwrap_or_default();
            for source in &sources {
                if let Influence::Timeline {
                    timeline,
                    held,
                    local,
                    ..
                } = source
                    && let TimelineOwner::Layer(owner) = &timeline.owner
                    && owner == path
                {
                    owned.push((timeline.index, *held, *local));
                }
            }
            let unfolded = self.expanded == Some(property);
            let line = row![
                text(tree::property_name(property)).size(13).width(80),
                text(value).size(13).width(Fill),
                text(badge).size(12),
            ]
            .spacing(8)
            .align_y(iced::Center);
            let mut b = button(line)
                .on_press(Message::Expand((!unfolded).then_some(property)))
                .width(Fill)
                .padding([2, 6])
                .style(button::text);
            if unfolded {
                b = b.style(button::secondary);
            }
            panel = panel.push(b);
            if unfolded {
                for (rank, source) in sources.iter().enumerate() {
                    panel = panel.push(
                        container(
                            text(format!("{}. {}", rank + 1, describe_source(source))).size(12),
                        )
                        .padding([0, 18]),
                    );
                }
            }
        }

        if !layer.bindings.is_empty() {
            panel = panel.push(container(text("BINDINGS").size(12)).padding([6, 0]));
            for binding in &layer.bindings {
                let mut line = format!(
                    "{} <- {}",
                    tree::property_name(binding.property),
                    binding.reading.variable
                );
                if binding.reading.map.is_some() {
                    line.push_str(", mapped");
                }
                if let Some(threshold) = binding.reading.threshold {
                    line.push_str(&format!(", from {threshold}"));
                }
                if let Some(debounce) = binding.reading.debounce {
                    line.push_str(&format!(", settled {debounce} s"));
                }
                if binding.scale != 1.0 {
                    line.push_str(&format!(", x {}", binding.scale));
                }
                if binding.offset != 0.0 {
                    line.push_str(&format!(", + {}", binding.offset));
                }
                if let Some(transition) = &binding.transition {
                    line.push_str(&format!(", over {} s", transition.duration));
                }
                panel = panel.push(text(line).size(13));
            }
        }

        if !layer.timelines.is_empty() {
            panel = panel.push(container(text("TIMELINES").size(12)).padding([6, 0]));
            for (index, timeline) in layer.timelines.iter().enumerate() {
                let mut starts: Vec<String> =
                    timeline.trigger.iter().map(|t| format!("on {t}")).collect();
                if let Some(when) = &timeline.when {
                    starts.push(format!("when {}", when.variable));
                }
                if let Some(whilst) = &timeline.whilst {
                    starts.push(format!("while {}", whilst.variable));
                }
                if timeline.autoplay {
                    starts.push("at load".to_owned());
                }
                let tracks: Vec<String> = timeline
                    .tracks
                    .iter()
                    .map(|t| tree::property_name(t.property))
                    .collect();
                let state = owned
                    .iter()
                    .find(|(i, _, _)| *i == index)
                    .map(|(_, held, local)| match (held, local) {
                        (true, _) => "held".to_owned(),
                        (false, Some(at)) => format!("running, {at:.2} s"),
                        (false, None) => "running".to_owned(),
                    })
                    .unwrap_or_default();
                panel = panel.push(
                    row![
                        text(timeline.name.clone()).size(13).width(Fill),
                        text(state).size(12)
                    ]
                    .spacing(8),
                );
                panel = panel.push(
                    container(
                        text(format!(
                            "{}{}: {}",
                            starts.join(", "),
                            if timeline.looping { ", looping" } else { "" },
                            tracks.join(", ")
                        ))
                        .size(12),
                    )
                    .padding([0, 12]),
                );
            }
        }

        panel = panel.push(container(text("JSON").size(12)).padding([6, 0]));
        let json = match self.written(show, path, layer) {
            Some(written) => written,
            None => {
                // The document does not have the layer where the engine
                // does: what the engine read, defaults and all.
                panel = panel.push(text("as the engine read it").size(12));
                serde_json::to_string_pretty(layer).unwrap_or_default()
            }
        };
        panel = panel.push(json_text(&json, &theme(self)));
        panel
    }

    /// The layer's text as the document has it, if the document has that
    /// layer at the layer's place.
    fn written(
        &self,
        show: &cuelight_core::Show,
        path: &LayerPath,
        layer: &Layer,
    ) -> Option<String> {
        let document = self.document.as_ref()?;
        let pointer = Pointer::parse(&tree::pointer(show, path)?).ok()?;
        let node = document.get(&pointer)?.value();
        // A part is named by its id.
        let named = node.get("name").or_else(|| node.get("id"))?;
        (named.as_str() == Some(layer.name.as_str())).then(|| document.text_at(&pointer))?
    }
}

/// JSON in the editor's mono font, coloured by token from the theme's
/// palette: keys, strings, numbers, literals and punctuation each their
/// own, whitespace and anything else in the text's colour.
fn json_text<'a>(json: &str, theme: &Theme) -> Element<'a, Message> {
    let palette = theme.palette();
    let text_l = palette.background.base.text.into_oklch().l;
    let back_l = palette.background.base.color.into_oklch().l;
    // A colour at a lightness this far from the text's towards the
    // background's, so it reads on the pane as text does, light on dark
    // or dark on light, whichever the theme is.
    let toward = |c: iced::Color, far: f32| {
        let mut oklch = c.into_oklch();
        oklch.l = text_l + (back_l - text_l) * far;
        Some(iced::Color::from_oklch(oklch))
    };
    // The palette's hues are picked to fill buttons; as text they are
    // lifted near the text's lightness. Dark text on light needs more
    // room for its hue to show.
    let near = if palette.is_dark { 0.25 } else { 0.4 };
    let colour = |token: Token| match token {
        Token::Key => toward(palette.primary.base.color, near),
        Token::String => toward(palette.success.base.color, near),
        Token::Number => toward(palette.warning.base.color, near),
        Token::Literal => toward(palette.danger.base.color, near),
        // The text's grey, halfway to the background: seen, not read.
        Token::Punctuation => toward(palette.background.base.text, 0.5),
        Token::Plain => None,
    };
    let spans: Vec<Span<'a, ()>> = syntax::tokens(json)
        .into_iter()
        .map(|(token, range)| span(json[range].to_owned()).color_maybe(colour(token)))
        .collect();
    rich_text(spans)
        .size(12)
        .font(iced::Font::new("DM Mono"))
        .into()
}

/// What an influence hands the property, as text.
fn influence_value(influence: &Influence) -> Option<String> {
    match influence {
        Influence::Base { value } => Some(inputs::show_value(value)),
        Influence::Binding { value, .. } => value.as_ref().map(inputs::show_value),
        Influence::Timeline { value, .. } => value.map(|v| inputs::show_value(&Value::Number(v))),
        _ => None,
    }
}

/// The badge of the source that wins right now.
fn winner(influence: &Influence) -> String {
    match influence {
        Influence::Base { .. } => "base".to_owned(),
        Influence::Binding { variable, .. } => format!("bound to {variable}"),
        Influence::Timeline { timeline, held, .. } => {
            if *held {
                format!("held by {}", timeline.name)
            } else {
                format!("timeline {}", timeline.name)
            }
        }
        _ => "?".to_owned(),
    }
}

/// One source of a property's value, in the unfolded list.
fn describe_source(influence: &Influence) -> String {
    let value = influence_value(influence)
        .map(|v| format!(" = {v}"))
        .unwrap_or_default();
    match influence {
        Influence::Base { .. } => format!("base{value}"),
        Influence::Binding {
            index, variable, ..
        } => format!("binding {} on {variable}{value}", index + 1),
        Influence::Timeline {
            timeline,
            local,
            held,
            ..
        } => {
            let at = match (held, local) {
                (true, _) => ", held".to_owned(),
                (false, Some(t)) => format!(" at {t:.2} s"),
                (false, None) => String::new(),
            };
            format!("timeline {}{at}{value}", timeline.name)
        }
        _ => format!("another source{value}"),
    }
}

impl App {
    /// The assets by kind, each with its thumbnail, its format and how
    /// often the show uses it; what else there is to know of one is in
    /// the inspector once it is picked.
    fn assets_panel<'a>(&'a self) -> Column<'a, Message> {
        const THUMB: f32 = 40.0;
        let mut panel = Column::new().spacing(4).padding(12);
        if self.library.is_empty() {
            return panel.push(text("This show ships no assets.").size(14));
        }
        let mut heading: Option<&str> = None;
        for (i, asset) in self.library.iter().enumerate() {
            if heading != Some(asset.kind.heading()) {
                heading = Some(asset.kind.heading());
                panel = panel.push(text(asset.kind.heading()).size(12));
            }
            let thumb: Element<'a, Message> = match &self.thumbs[i] {
                Some(Thumb::Image(handle)) => image(handle.clone())
                    .width(THUMB)
                    .height(THUMB)
                    .content_fit(ContentFit::Contain)
                    .filter_method(image::FilterMethod::Nearest)
                    .into(),
                Some(Thumb::Svg(handle)) => svg(handle.clone())
                    .width(THUMB)
                    .height(THUMB)
                    .content_fit(ContentFit::Contain)
                    .into(),
                None => container(text(kind_mark(asset.kind)).size(16))
                    .width(THUMB)
                    .height(THUMB)
                    .center_x(THUMB)
                    .center_y(THUMB)
                    .into(),
            };
            let mut facts = asset.summary();
            let playing = self.preview.as_ref().is_some_and(|p| p.index == i);
            if playing {
                facts.push_str(", playing");
            }
            let mut about = column![text(&asset.name).size(14), text(facts).size(12)].spacing(2);
            if let Some(faces) = &self.faces[i] {
                // The sample at the size the show uses the font at, as
                // tall as a row allows; what does not fit is cut off.
                about = about.push(
                    container(self.face(&faces.sample, specimen::SAMPLE, 1.0, Some(SAMPLE_HEIGHT)))
                        .width(Fill)
                        .clip(true),
                );
            }
            let line = row![thumb, about].spacing(8).align_y(iced::Center);
            let mut b = button(line)
                .on_press(Message::Select(Some(i)))
                .width(Fill)
                .style(button::text);
            if self.selected == Some(i) {
                b = b.style(button::secondary);
            }
            if asset.kind == Kind::Sound {
                // Played once from here, outside the show's clock; the
                // same press stops it. Nothing to press when there is no
                // sound to be had.
                let mut play = button(text(if playing { "stop" } else { "play" }).size(12))
                    .style(button::secondary);
                if self.can_play() {
                    play = play.on_press(Message::Preview(i));
                }
                panel = panel.push(row![b, play].spacing(4).align_y(iced::Center));
            } else {
                panel = panel.push(b);
            }
        }
        panel
    }
}

impl App {
    /// The picked artwork, large: fitted to the pane or at its own size,
    /// on the show's background.
    fn preview<'a>(&'a self, session: &'a Session, i: usize, size: Size) -> Column<'a, Message> {
        const PADDING: f32 = 12.0;
        /// What a scrollbar covers, as on the stage: room past the
        /// artwork's bottom edge while it scrolls sideways.
        const SCROLLBAR: f32 = 10.0;
        let asset = &self.library[i];
        let mut panel = Column::new().spacing(4).padding(PADDING);
        panel = panel.push(text(&asset.name).size(16));
        let facts = match (asset.kind, asset.size) {
            (Kind::Image, Some([w, h])) => format!("image, {w} x {h} px"),
            (Kind::Vector, Some([w, h])) => format!("vector artwork, {w} x {h}"),
            (Kind::Vector, _) => "vector artwork".to_owned(),
            _ => "image".to_owned(),
        };
        panel = panel.push(text(facts).size(12));

        if let (Some(thumb), Some([w, h])) = (&self.thumbs[i], asset.size) {
            let (w, h) = (w.max(1.0) as f32, h.max(1.0) as f32);
            let room = Size::new(
                (size.width - 2.0 * PADDING).max(1.0),
                (size.height - 2.0 * PADDING).max(1.0),
            );
            let fit = (room.width / w).min(room.height / h);
            let scale = if self.actual_size { 1.0 } else { fit };
            let (drawn_w, drawn_h) = ((w * scale).round().max(1.0), (h * scale).round().max(1.0));

            let size_button = |label: &'a str, actual: bool| {
                let mut b = button(text(label).size(13)).on_press(Message::ActualSize(actual));
                if self.actual_size == actual {
                    b = b.style(button::secondary);
                }
                b
            };
            panel = panel.push(
                container(
                    row![
                        size_button("Fit", false),
                        size_button("100%", true),
                        text(format!("{:.0}%", scale * 100.0)).size(13),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                )
                .padding([6, 0]),
            );

            let art: Element<'a, Message> = match thumb {
                Thumb::Image(handle) => image(handle.clone())
                    .width(drawn_w)
                    .height(drawn_h)
                    .content_fit(ContentFit::Fill)
                    // Enlarged pixels stay square; reduced ones blend.
                    .filter_method(if scale >= 1.0 {
                        image::FilterMethod::Nearest
                    } else {
                        image::FilterMethod::Linear
                    })
                    .into(),
                Thumb::Svg(handle) => svg(handle.clone())
                    .width(drawn_w)
                    .height(drawn_h)
                    .content_fit(ContentFit::Fill)
                    .into(),
            };
            let backdrop = session
                .engine
                .lock()
                .expect("the engine is not poisoned")
                .show()
                .and_then(|show| cuelight_core::parse_color(&show.background))
                .map(|[r, g, b, a]| iced::Color::from_rgba8(r, g, b, f32::from(a) / 255.0))
                .unwrap_or(iced::Color::BLACK);
            let framed = container(art).style(move |_| container::Style {
                background: Some(backdrop.into()),
                ..container::Style::default()
            });
            panel = panel.push(if drawn_w > room.width {
                Element::from(
                    scrollable(container(framed).padding(iced::Padding {
                        bottom: SCROLLBAR,
                        ..iced::Padding::ZERO
                    }))
                    .direction(Direction::Horizontal(Scrollbar::default()))
                    .width(Fill),
                )
            } else {
                framed.into()
            });
        }

        panel
    }
}

impl App {
    /// A picked asset: its preview or specimen, then its file, then every
    /// layer that uses it, each a link to that layer; for vector artwork,
    /// last, the element ids it is made of and which the show moves as
    /// parts.
    fn asset_panel<'a>(
        &'a self,
        session: &'a Session,
        i: usize,
        size: Size,
    ) -> Column<'a, Message> {
        let asset = &self.library[i];
        let mut panel = if let Some(faces) = &self.faces[i] {
            self.specimen_panel(session, asset, faces)
        } else if matches!(asset.kind, Kind::Image | Kind::Vector) {
            self.preview(session, i, size)
        } else {
            let engine = session.engine.lock().expect("the engine is not poisoned");
            let facts = match asset.kind {
                Kind::Sound => engine
                    .sound_duration(&asset.name)
                    .map(|d| format!("sound, {d:.2} s")),
                Kind::Video => engine
                    .video(&asset.name)
                    .map(|v| format!("video, {:.2} s, {} x {}", v.duration, v.width, v.height)),
                _ => None,
            };
            column![
                text(&asset.name).size(16),
                text(facts.unwrap_or_else(|| asset.kind.name().to_owned())).size(12)
            ]
            .spacing(4)
            .padding(12)
        };
        let heading = |label: &'a str| container(text(label).size(12)).padding([6, 0]);

        panel = panel.push(heading("FILE"));
        match &asset.file {
            Some(file) => {
                panel = panel.push(text(file).size(13));
                let mut about = asset.format().unwrap_or_default();
                if let Some(bytes) = asset.bytes {
                    about = format!("{about}, {}", assets::file_size(bytes));
                }
                panel = panel.push(text(about).size(12));
            }
            None => panel = panel.push(text("no file: the show came without its folder").size(12)),
        }

        panel = panel.push(heading("USED BY"));
        if asset.uses.is_empty() {
            panel = panel.push(text("nothing in this show").size(13));
            if let Some(file) = &asset.file {
                let prefix = format!("{file}: ");
                for line in session.log.audit_of(file) {
                    let said = line.text.strip_prefix(&prefix).unwrap_or(&line.text);
                    panel = panel.push(text(format!("{}: {said}", line.kind.label())).size(12));
                }
            }
        }
        for used in &asset.uses {
            panel = panel.push(
                button(column![text(&used.place).size(13), text(&used.how).size(12)].spacing(2))
                    .on_press(Message::Choose(used.path.clone()))
                    .width(Fill)
                    .padding([2, 6])
                    .style(button::text),
            );
        }

        if let Some(structure) = &asset.structure {
            panel = panel.push(container(text("ELEMENTS").size(12)).padding([6, 0]));
            panel = panel.push(
                text(format!(
                    "{} path(s), {} inside no id",
                    structure.paths, structure.loose
                ))
                .size(12),
            );
            if structure.elements.is_empty() {
                panel = panel.push(
                    text("No element carries an id: the artwork moves only as a whole.").size(13),
                );
            }
            for element in &structure.elements {
                let indent = element.depth as f32 * 14.0;
                panel = panel.push(
                    container(
                        row![
                            text(&element.id).size(13).width(Fill),
                            text(format!("{} path(s)", element.paths)).size(12),
                        ]
                        .spacing(6)
                        .align_y(iced::Center),
                    )
                    .padding(iced::Padding::ZERO.left(indent)),
                );
                if !element.parts.is_empty() {
                    panel = panel.push(
                        container(text(format!("part in {}", element.parts.join(", "))).size(12))
                            .padding(iced::Padding::ZERO.left(indent + 12.0)),
                    );
                }
            }
            if !structure.unknown.is_empty() {
                panel =
                    panel.push(container(text("PARTS IT DOES NOT HAVE").size(12)).padding([6, 0]));
                for (id, place) in &structure.unknown {
                    panel = panel.push(
                        row![
                            text(id).size(13).width(Fill),
                            text(format!("named by {place}")).size(12)
                        ]
                        .spacing(6),
                    );
                }
            }
        }
        panel
    }
}

/// The tallest a font's sample line is drawn in its row.
const SAMPLE_HEIGHT: f32 = 48.0;

impl App {
    /// A picked font: every printable character at each size the show
    /// uses the font at, with the styles that use it so, at 100% and
    /// zoomed in.
    fn specimen_panel<'a>(
        &'a self,
        session: &'a Session,
        asset: &'a Asset,
        faces: &'a Faces,
    ) -> Column<'a, Message> {
        let engine = session.engine.lock().expect("the engine is not poisoned");
        let mut panel = Column::new().spacing(4).padding(12);
        panel = panel.push(text(&asset.name).size(16));
        let kind = match faces.sizings[0].size {
            None => "bitmap font",
            Some(_) => "outline font",
        };
        panel = panel.push(text(kind).size(12));
        for (sizing, lines) in faces.sizings.iter().zip(&faces.specimens) {
            panel = panel.push(space::vertical().height(8));
            panel = panel.push(text(sizing_name(sizing).to_uppercase()).size(12));
            if lines
                .iter()
                .any(|(_, _, face)| matches!(face, Face::Outline { .. }))
            {
                // The stage fills outlines through the renderer; here
                // they are iced's, in the font the file declares.
                panel = panel.push(text("outlines, drawn by the editor in this font").size(12));
            }
            if sizing.styles.is_empty() {
                panel = panel.push(text("no font style uses it").size(12));
            }
            for name in &sizing.styles {
                let style = engine.show().and_then(|show| show.fonts.get(name));
                panel = panel.push(
                    row![
                        text(name).size(13),
                        text(style.map(style_name).unwrap_or_default()).size(12)
                    ]
                    .spacing(8),
                );
            }
            // Zoomed so a line is some 24 pixels tall, for a font smaller
            // than that: pixels are looked at close.
            let tall = lines
                .iter()
                .find_map(|(_, _, face)| match face {
                    Face::Image { height, .. } => Some(*height as f32),
                    Face::Outline { size, .. } => Some(*size),
                    Face::Nothing => None,
                })
                .unwrap_or(16.0)
                .max(1.0);
            let zoom = (24.0 / tall).ceil().min(8.0);
            let zooms: &[f32] = if zoom >= 2.0 { &[1.0, zoom] } else { &[1.0] };
            for &zoom in zooms {
                let mut block = Column::new().spacing(2);
                for (code, line, face) in lines {
                    block = block.push(
                        row![
                            text(code).size(12).font(Font::MONOSPACE).width(24),
                            self.face(face, line, zoom, None)
                        ]
                        .spacing(8)
                        .align_y(iced::Center),
                    );
                }
                panel = panel.push(text(format!("{:.0}%", zoom * 100.0)).size(12));
                panel = panel.push(
                    scrollable(block)
                        .direction(Direction::Horizontal(Scrollbar::default().spacing(4)))
                        .width(Fill),
                );
            }
        }
        panel
    }

    /// `line` in a show's font, `zoom` times its size or at most `max`
    /// tall: the engine's pixels as they are, never smoothed while they
    /// are enlarged; an outline font's text once iced has the font.
    fn face<'a>(
        &self,
        face: &Face,
        line: &'a str,
        zoom: f32,
        max: Option<f32>,
    ) -> Element<'a, Message> {
        match face {
            Face::Image {
                handle,
                width,
                height,
            } => {
                let (width, height) = (*width as f32, *height as f32);
                let scale = max.map_or(zoom, |max| zoom.min(max / height));
                // Drawn from its left edge at its own size times the
                // scale; a narrower place cuts it off rather than
                // shrinking it.
                image(handle.clone())
                    .width(width * scale)
                    .height(height * scale)
                    .content_fit(ContentFit::None)
                    .scale(scale)
                    .filter_method(if scale >= 1.0 {
                        image::FilterMethod::Nearest
                    } else {
                        image::FilterMethod::Linear
                    })
                    .into()
            }
            Face::Outline { font, name, size } => {
                if !self.loaded.contains(name) {
                    return text("the font is not loaded").size(12).into();
                }
                let size = max.map_or(size * zoom, |max| (size * zoom).min(max));
                text(line)
                    .font(*font)
                    .size(size)
                    .wrapping(Wrapping::None)
                    .into()
            }
            Face::Nothing => text("none of these characters").size(12).into(),
        }
    }
}

/// How a font is sized: a bitmap font's own size, or pixels per em and
/// whether they are drawn as exact pixels.
fn sizing_name(sizing: &Sizing) -> String {
    match sizing.size {
        None => "bitmap, its own size".to_owned(),
        Some(size) if sizing.pixels => format!("{size} px, as pixels"),
        Some(size) => format!("{size} px"),
    }
}

/// What a font style adds to its font: colour, border and shadow.
fn style_name(style: &cuelight_core::FontStyle) -> String {
    let mut name = style.color.clone();
    if let Some(border) = &style.border {
        name.push_str(&format!(", border {} {} px", border.color, border.width));
    }
    if let Some(shadow) = &style.shadow {
        name.push_str(&format!(
            ", shadow {} at {}, {}",
            shadow.color, shadow.offset[0], shadow.offset[1]
        ));
    }
    name
}

/// How each font in a library looks, drawn by the engine once when the
/// show opens.
fn faces(
    engine: &cuelight::Engine,
    files: &BTreeMap<String, Vec<u8>>,
    library: &[Asset],
) -> Vec<Option<Faces>> {
    library
        .iter()
        .map(|asset| {
            if asset.kind != Kind::Font {
                return None;
            }
            let show = engine.show()?;
            let outline = engine.outline_fonts().any(|(name, _)| name == asset.name);
            let look = specimen::look(files, show, &asset.name, outline);
            let face = |drawn: Option<Drawn>, sizing: &Sizing| match drawn {
                Some(Drawn::Raster(raster)) => Face::Image {
                    handle: image::Handle::from_rgba(
                        raster.width,
                        raster.height,
                        raster.pixels.to_vec(),
                    ),
                    width: raster.width,
                    height: raster.height,
                },
                Some(Drawn::Outline(Some(family))) => Face::Outline {
                    font: font(&family),
                    name: asset.name.clone(),
                    size: sizing.size.unwrap_or(16.0) as f32,
                },
                _ => Face::Nothing,
            };
            let sample = face(look.sample, &look.sizings[0]);
            let specimens = look
                .specimens
                .into_iter()
                .zip(&look.sizings)
                .map(|(lines, sizing)| {
                    specimen::rows()
                        .into_iter()
                        .zip(lines)
                        .map(|((code, _), line)| {
                            let drawn = face(line.drawn, sizing);
                            (code, line.text, drawn)
                        })
                        .collect()
                })
                .collect();
            Some(Faces {
                sizings: look.sizings,
                sample,
                specimens,
            })
        })
        .collect()
}

/// The iced font for an outline font's face: its family at the
/// nearest of iced's weights, italic when it slants.
fn font(family: &specimen::Family) -> Font {
    use iced::font::{Style, Weight};
    const WEIGHTS: [Weight; 9] = [
        Weight::Thin,
        Weight::ExtraLight,
        Weight::Light,
        Weight::Normal,
        Weight::Medium,
        Weight::Semibold,
        Weight::Bold,
        Weight::ExtraBold,
        Weight::Black,
    ];
    let step = (usize::from(family.weight.clamp(100, 900)) + 50) / 100 - 1;
    Font::with_family(family.name.as_str())
        .weight(WEIGHTS[step])
        .style(if family.italic {
            Style::Italic
        } else {
            Style::Normal
        })
}

/// Give iced the show's outline fonts, so their samples can be drawn in
/// them.
fn load_fonts(engine: &cuelight::Engine, library: &[Asset]) -> Task<Message> {
    let loads: Vec<Task<Message>> = engine
        .outline_fonts()
        .filter(|(name, _)| {
            library
                .iter()
                .any(|asset| asset.kind == Kind::Font && asset.name == *name)
        })
        .map(|(name, bytes)| {
            let name = name.to_owned();
            iced::font::load(bytes.to_vec()).map(move |result| {
                if let Err(error) = &result {
                    log::warn!("font {name}: {error:?}");
                }
                Message::FontLoaded(name.clone(), result.is_ok())
            })
        })
        .collect();
    Task::batch(loads)
}

/// Thumbnails for the artwork in a library: an image's pixels as the
/// engine decoded them, an SVG's bytes as the show shipped them.
fn thumbs(engine: &cuelight::Engine, library: &[Asset]) -> Vec<Option<Thumb>> {
    library
        .iter()
        .map(|asset| match asset.kind {
            Kind::Image => engine.image(&asset.name).map(|i| {
                Thumb::Image(image::Handle::from_rgba(
                    i.width,
                    i.height,
                    i.pixels.to_vec(),
                ))
            }),
            Kind::Vector => asset
                .svg
                .as_ref()
                .map(|bytes| Thumb::Svg(svg::Handle::from_memory(bytes.to_vec()))),
            _ => None,
        })
        .collect()
}

/// A stand-in for a thumbnail, for the kinds that have none.
fn kind_mark(kind: Kind) -> &'static str {
    match kind {
        Kind::Image | Kind::Vector => "?",
        Kind::Font => "Aa",
        Kind::Sound => "))",
        Kind::Video => ">",
    }
}

/// The window's pixels as a PNG.
#[cfg(not(target_arch = "wasm32"))]
fn write_png(path: &std::path::Path, shot: &iced::window::Screenshot) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        shot.size.width,
        shot.size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&shot.rgba)?;
    writer.finish()?;
    Ok(())
}

/// A key's name as a browser gives it, which is how a show names one.
fn key_name(key: &keyboard::Key) -> String {
    match key {
        keyboard::Key::Character(c) => c.to_string(),
        keyboard::Key::Named(keyboard::key::Named::Space) => " ".to_owned(),
        keyboard::Key::Named(named) => format!("{named:?}"),
        keyboard::Key::Unidentified => String::new(),
    }
}

impl App {
    /// One heading of trigger buttons. A button names the keys that fire
    /// its trigger; a section whose scene is not up is drawn dimmed.
    fn triggers<'a>(
        &'a self,
        panel: Column<'a, Message>,
        heading: String,
        triggers: &[&'a str],
        live: bool,
    ) -> Column<'a, Message> {
        let mut panel = panel.push(text(heading).size(12));
        for trigger in triggers {
            let keys: Vec<&str> = self
                .inputs
                .keys
                .iter()
                .filter(|(_, t)| t == trigger)
                .map(|(k, _)| if k == " " { "Space" } else { k.as_str() })
                .collect();
            let label = if keys.is_empty() {
                (*trigger).to_owned()
            } else {
                format!("{trigger}  [{}]", keys.join(", "))
            };
            let mut b = button(text(label).size(14))
                .on_press(Message::Fire((*trigger).to_owned()))
                .width(Fill);
            if !live {
                b = b.style(button::secondary);
            }
            panel = panel.push(b);
        }
        panel
    }
}

fn summary(summary: &Summary) -> Column<'_, Message> {
    let mut rows = Column::new().spacing(6).padding(16);
    for (label, value) in opened::lines(summary) {
        rows = rows.push(column![text(label).size(12), text(value).size(14)].spacing(2));
    }
    if !summary.problems.is_empty() {
        rows = rows.push(text(format!("{} problem(s)", summary.problems.len())).size(14));
        for problem in &summary.problems {
            rows = rows.push(text(problem).size(13));
        }
    }
    rows
}

/// The theme iced draws the window in: the one it picks for the
/// system's preference (or `ICED_THEME`), as the colours of the view that
/// are not a widget's own style follow it.
pub fn theme(app: &App) -> Theme {
    <Theme as iced::theme::Base>::default(app.mode)
}

/// Ask for the system's light or dark preference.
fn system_mode() -> Task<Message> {
    iced::system::theme().map(Message::Mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuelight_core::Value;
    use cuelight_editor_core::session::{Step, What};
    use iced_test::simulator;

    #[test]
    fn the_command_line_is_read() {
        use clap::Parser;
        let options = Options::try_parse_from([
            "cuelight-editor",
            "deck",
            "--zoom",
            "2",
            "--pick",
            "10, 20",
            "--asset",
            "robot",
            "--silent",
            "--screenshot",
            "out.png",
        ])
        .unwrap();
        assert_eq!(
            options,
            Options {
                show: Some("deck".into()),
                zoom: Some(2.0),
                pick: Some([10.0, 20.0]),
                asset: Some("robot".to_owned()),
                trigger: Vec::new(),
                silent: true,
                screenshot: Some("out.png".into()),
            }
        );
        assert!(Options::try_parse_from(["cuelight-editor", "--pick", "10"]).is_err());
        assert!(Options::try_parse_from(["cuelight-editor", "--zoom", "big"]).is_err());
    }

    #[test]
    fn starts_with_an_open_button_and_a_hint() {
        let (app, _) = App::new();
        let mut ui = simulator(app.view());
        assert!(ui.find("Open file...").is_ok());
        assert!(ui.find("Open a show folder, a packed show (.cuelight) or a show.json, or drop one on this window.").is_ok());
    }

    #[test]
    fn shows_what_it_opened() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        assert!(app.status.starts_with("opened "), "{}", app.status);
        let mut ui = simulator(app.view());
        assert!(ui.find("mini (format 1)").is_ok());
        assert!(ui.find("64 x 32").is_ok());
        assert!(ui.find("Play").is_ok(), "an opened show is paused at 0");
    }

    #[test]
    fn the_stage_zooms_and_fits_again() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        assert_eq!(app.zoom, Zoom::Fit);
        let mut ui = simulator(app.view());
        assert!(ui.find("Fit").is_ok());
        let _ = ui.click("100%");
        for message in ui.into_messages() {
            let _ = app.update(message);
        }
        assert_eq!(app.zoom, Zoom::Scale(1.0));
        let _ = app.update(Message::ZoomBy(Zoom::STEP));
        assert_eq!(app.zoom, Zoom::Scale(Zoom::STEP));
        {
            let mut ui = simulator(app.view());
            assert!(ui.find("125%").is_ok(), "the bar shows the scale");
        }
        let _ = app.update(Message::KeyPressed(
            keyboard::Key::Character("f".into()),
            keyboard::Modifiers::empty(),
        ));
        assert_eq!(app.zoom, Zoom::Fit);
        // Zooming out of a fit starts from the fitted scale, which the
        // layout found.
        {
            let mut ui = simulator(app.view());
            let _ = ui.find("Fit");
        }
        let fitted = app.fitted.get();
        assert!(fitted > 0.0 && fitted != 1.0, "{fitted}");
        let _ = app.update(Message::ZoomBy(1.0 / Zoom::STEP));
        assert_eq!(app.zoom, Zoom::Scale(fitted / Zoom::STEP));
    }

    #[test]
    fn the_zoom_stops_where_vello_stops_drawing() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        // A 64 x 32 show zooms all the way.
        let _ = app.update(Message::ZoomBy(100.0));
        assert_eq!(app.zoom, Zoom::Scale(Zoom::MAX));
        // A 4000 x 4000 one on a 2x screen stops near 4096 physical pixels.
        app.summary.size = [4000, 4000];
        let _ = app.update(Message::Rescaled(2.0));
        let _ = app.update(Message::Zoom(Zoom::Scale(1.0)));
        let Zoom::Scale(scale) = app.zoom else {
            panic!("a scale");
        };
        assert!((0.45..=0.512).contains(&scale), "{scale}");
        assert!(crate::stage::drawable([
            (4000.0 * scale * 2.0).round() as u32,
            (4000.0 * scale * 2.0).round() as u32
        ]));
    }

    #[test]
    fn the_splits_between_the_areas_move() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let pane_grid::Node::Split { id, ratio, .. } = *app.panes.layout() else {
            panic!("the areas are split");
        };
        let _ = app.update(Message::Resized(pane_grid::ResizeEvent {
            split: id,
            ratio: 0.4,
        }));
        let pane_grid::Node::Split { ratio: now, .. } = *app.panes.layout() else {
            panic!("still split");
        };
        assert_ne!(ratio, now);
        assert!((now - 0.4).abs() < 1e-6);
        let mut ui = simulator(app.view());
        assert!(ui.find("Fit").is_ok(), "the stage is in its pane");
        assert!(
            ui.find("mini (format 1)").is_ok(),
            "the summary is in its pane"
        );
    }

    #[test]
    fn the_library_lists_the_assets_and_where_they_are_used() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let _ = app.update(Message::Tab(Tab::Assets));
        {
            let mut ui = simulator(app.view());
            assert!(ui.find("ARTWORK").is_ok());
            assert!(ui.find("dot").is_ok());
            assert!(ui.find("png, used once").is_ok(), "its format and uses");
            assert!(ui.find("USED BY").is_err(), "nothing picked yet");
        }
        assert!(matches!(app.thumbs.as_slice(), [Some(Thumb::Image(_))]));
        let _ = app.update(Message::Select(Some(0)));
        {
            let mut ui = simulator(app.view());
            assert!(ui.find("USED BY").is_ok(), "in the inspector");
            assert!(ui.find("assets/dot.png").is_ok(), "the file");
            assert!(ui.find("image layer").is_ok());
            // A use is a link to the layer, shown where the tree has it.
            let _ = ui.click("group/dot");
            for message in ui.into_messages() {
                let _ = app.update(message);
            }
        }
        assert_eq!(app.tab, Tab::Layers);
        assert_eq!(app.selected, None);
        assert_eq!(
            app.selection,
            [LayerPath::new(cuelight_core::Root::Show, [1, 0])]
        );
        let mut ui = simulator(app.view());
        assert!(ui.find("image, group/dot").is_ok(), "the layer inspected");
    }

    #[test]
    fn a_picked_image_is_previewed_until_a_layer_is_picked() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let _ = app.update(Message::Reveal("dot".to_owned()));
        assert_eq!((app.tab, app.selected), (Tab::Assets, Some(0)));
        {
            let mut ui = simulator(app.view());
            assert!(ui.find("image, 8 x 8 px").is_ok(), "the inspector has it");
        }
        let _ = app.update(Message::ActualSize(true));
        {
            let mut ui = simulator(app.view());
            assert!(ui.find("image, 8 x 8 px").is_ok());
        }
        // A layer picked on the stage takes the inspector back.
        let _ = app.update(Message::Pick([32.0, 14.0], Pick::default()));
        assert_eq!(app.selected, None);
        {
            let mut ui = simulator(app.view());
            assert!(ui.find("image, 8 x 8 px").is_err());
            assert!(ui.find("image, group/dot").is_ok());
        }
        // An asset the show does not have is said, not picked.
        let _ = app.update(Message::Reveal("nothing".to_owned()));
        assert_eq!(app.selected, None);
        assert!(app.status.contains("no asset"), "{}", app.status);
    }

    #[test]
    fn a_font_shows_its_face_in_its_row_and_a_specimen_when_picked() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/typed"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let _ = app.update(Message::Reveal("tiny".to_owned()));
        assert!(matches!(
            app.faces.as_slice(),
            [Some(Faces {
                sample: Face::Image { height: 3, .. },
                ..
            })]
        ));
        let mut ui = simulator(app.view());
        assert!(
            ui.find("fnt, used 2 times").is_ok(),
            "the row's format and uses"
        );
        assert!(ui.find("BITMAP, ITS OWN SIZE").is_ok(), "the specimen's");
        assert!(ui.find("loud").is_ok(), "a style using it");
        assert!(ui.find("#FF0000, border #000000 1 px").is_ok());
        assert!(ui.find("100%").is_ok());
        assert!(ui.find("800%").is_ok(), "3 pixels tall, zoomed to 24");
        assert!(ui.find("none of these characters").is_ok(), "no digits");
    }

    /// The JSON is the layer as the file has it: the author's own
    /// layout, and nothing the engine fills in.
    #[test]
    fn the_json_is_the_layers_own_text() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let _ = app.update(Message::Pick([32.0, 14.0], Pick::default()));
        let session = app.session.as_ref().unwrap();
        let engine = session.engine.lock().unwrap();
        let show = engine.show().unwrap();
        let path = app.selection.last().unwrap();
        let layer = tree::layer(show, path).unwrap();
        let written = app.written(show, path, layer).unwrap();
        assert!(
            written.starts_with(r#"{ "name": "dot", "type": "image", "image": "dot""#),
            "{written}"
        );
        assert!(
            written.contains("\n  \"timelines\": ["),
            "moved left: {written}"
        );
        assert!(written.ends_with("\n}"), "{written}");
        assert!(!written.contains("\"visible\""), "no defaults: {written}");
    }

    #[test]
    fn clicking_the_stage_picks_the_layer_that_drew_it() {
        use cuelight_core::Root;
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let dot = LayerPath::new(Root::Show, [1, 0]);
        let floor = LayerPath::new(Root::Show, [0]);
        // The dot is drawn at the group's place; the floor along the bottom.
        let _ = app.update(Message::Pick([32.0, 14.0], Pick::default()));
        assert_eq!(app.selection, std::slice::from_ref(&dot));
        let _ = app.update(Message::Pick([10.0, 27.0], Pick::default()));
        assert_eq!(app.selection, std::slice::from_ref(&floor));
        // Shift adds; a click on nothing clears; Escape clears.
        let _ = app.update(Message::Pick(
            [32.0, 14.0],
            Pick {
                alt: false,
                shift: true,
            },
        ));
        assert_eq!(app.selection, [floor.clone(), dot.clone()]);
        let _ = app.update(Message::Pick([1.0, 1.0], Pick::default()));
        assert!(app.selection.is_empty());
        let _ = app.update(Message::Choose(dot.clone()));
        let _ = app.update(Message::KeyPressed(
            keyboard::Key::Named(keyboard::key::Named::Escape),
            keyboard::Modifiers::empty(),
        ));
        assert!(app.selection.is_empty());
        // The tree shows both, and picking there works too.
        let mut ui = simulator(app.view());
        assert!(ui.find("SHOW").is_ok());
        assert!(ui.find("dot").is_ok());
        assert!(ui.find("group").is_ok());
    }

    #[test]
    fn the_inspector_says_where_a_value_comes_from() {
        use cuelight_core::Root;
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let _ = app.update(Message::Choose(LayerPath::new(Root::Show, [1, 0])));
        {
            let mut ui = simulator(app.view());
            assert!(ui.find("PLACEMENT").is_ok());
            assert!(ui.find("bound to lit").is_ok(), "opacity is bound");
            assert!(ui.find("image, group/dot").is_ok());
            assert!(ui.find("hop").is_ok(), "the timeline is listed");
        }
        // Once `go` starts the hop, y is the timeline's.
        let _ = app.update(Message::Fire("go".to_owned()));
        let _ = app.update(Message::TogglePause);
        let start = Instant::now();
        let _ = app.update(Message::Tick(start));
        let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(100)));
        let _ = app.update(Message::Expand(Some(Property::Y)));
        let mut ui = simulator(app.view());
        assert!(ui.find("timeline hop").is_ok(), "y is owned by the hop");
        assert!(ui.find("2. base = 0").is_ok(), "the base value is last");
    }

    #[test]
    fn a_tick_moves_the_show_and_pausing_holds_it() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let _ = app.update(Message::TogglePause);
        let start = Instant::now();
        let _ = app.update(Message::Tick(start));
        let from = app.session.as_ref().unwrap().time;
        let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(500)));
        let time = app.session.as_ref().unwrap().time;
        assert!((time - from - 0.5).abs() < 1e-6, "{from} -> {time}");
        let _ = app.update(Message::TogglePause);
        let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(900)));
        assert_eq!(
            app.session.as_ref().unwrap().time,
            time,
            "paused shows stand still"
        );
    }

    #[test]
    fn seeking_and_stepping_land_where_asked() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let _ = app.update(Message::Seek(1.25));
        let session = app.session.as_ref().unwrap();
        assert!((session.time - 1.25).abs() < 1e-9);
        assert!(session.paused, "dragging the playhead pauses");
        assert_eq!(
            session.engine.lock().unwrap().time(),
            1.25,
            "the engine is there too"
        );
        let _ = app.update(Message::Step(-1.0 / 60.0));
        let session = app.session.as_ref().unwrap();
        assert!((session.time - (1.25 - 1.0 / 60.0)).abs() < 1e-9);
        let _ = app.update(Message::Step(-5.0));
        assert_eq!(
            app.session.as_ref().unwrap().time,
            0.0,
            "a step back stops at the start"
        );
    }

    #[test]
    fn a_variable_set_by_hand_is_replayed_and_driver_steps_are_logged() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let lit = |app: &App| app.session.as_ref().unwrap().value("lit");

        // The driver lights the dot at 0.5 s; a hand lights it at 0.3 s.
        let _ = app.update(Message::Seek(0.3));
        let _ = app.update(Message::Set("lit".into(), "true".into()));
        assert_eq!(lit(&app), Some(Value::Bool(true)));
        let _ = app.update(Message::Seek(0.0));
        assert_eq!(
            lit(&app),
            Some(Value::Bool(false)),
            "before the hand set it"
        );
        let _ = app.update(Message::Seek(0.4));
        assert_eq!(
            lit(&app),
            Some(Value::Bool(true)),
            "a scrub replays the set"
        );

        // Played, the driver's steps are logged at their own instants.
        let _ = app.update(Message::Seek(0.0));
        let _ = app.update(Message::TogglePause);
        let start = Instant::now();
        let _ = app.update(Message::Tick(start));
        let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(600)));
        let session = app.session.as_ref().unwrap();
        let driver: Vec<_> = session
            .happened
            .iter()
            .filter(|h| matches!(h.what, What::Driver(_)))
            .collect();
        assert_eq!(driver.len(), 2, "{:?}", session.happened);
        assert_eq!(driver[0].at, 0.5);
        assert!(
            matches!(&driver[0].what, What::Driver(Step::Trigger { trigger }) if trigger == "go")
        );
        assert!(matches!(&driver[1].what, What::Driver(Step::Set { .. })));
    }

    #[test]
    fn keys_presses_and_buttons_reach_the_show() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        assert!(app.inputs.triggers.contains("go"));
        assert_eq!(app.inputs.keys.get(" ").map(String::as_str), Some("go"));

        // The show maps Space to `go`, so Space fires it rather than pausing.
        let _ = app.update(Message::KeyPressed(
            keyboard::Key::Named(keyboard::key::Named::Space),
            keyboard::Modifiers::empty(),
        ));
        let session = app.session.as_ref().unwrap();
        assert!(
            session.paused,
            "an input by hand leaves a paused show paused"
        );
        assert!(
            matches!(session.happened.back().map(|h| &h.what), Some(What::Fired(t)) if t == "go")
        );

        // With Ctrl, the editor's own Space plays.
        let _ = app.update(Message::KeyPressed(
            keyboard::Key::Named(keyboard::key::Named::Space),
            keyboard::Modifiers::CTRL,
        ));
        assert!(!app.session.as_ref().unwrap().paused);

        // A press on the dot fires `go`; one on the floor fires nothing.
        let _ = app.update(Message::Press([32.0, 14.0]));
        let _ = app.update(Message::Press([10.0, 27.0]));
        let fired = app
            .session
            .as_ref()
            .unwrap()
            .happened
            .iter()
            .filter(|h| matches!(&h.what, What::Fired(t) if t == "go"))
            .count();
        assert_eq!(fired, 2);

        let _ = app.update(Message::Set("lit".to_owned(), "true".to_owned()));
        assert_eq!(
            app.session.as_ref().unwrap().value("lit"),
            Some(cuelight_core::Value::Bool(true))
        );
        let _ = app.update(Message::Fire("go".to_owned()));
        let mut ui = simulator(app.view());
        assert!(
            ui.find("go  [Space]").is_ok(),
            "the trigger's button names its key"
        );
        assert!(
            ui.find("ANYWHERE").is_ok(),
            "a trigger only the show's own layers hear is listed as heard anywhere"
        );
    }

    #[test]
    fn without_the_driver_the_show_waits_for_the_hand() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let _ = app.update(Message::Drive(false));
        // The driver sets `lit` at 0.5 s; without it nothing does.
        let _ = app.update(Message::Seek(1.0));
        assert_eq!(
            app.session.as_ref().unwrap().value("lit"),
            Some(cuelight_core::Value::Bool(false))
        );
        let _ = app.update(Message::Drive(true));
        let _ = app.update(Message::Seek(1.0));
        assert_eq!(
            app.session.as_ref().unwrap().value("lit"),
            Some(cuelight_core::Value::Bool(true))
        );
    }
}
