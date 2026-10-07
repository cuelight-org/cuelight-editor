//! The window: an open bar with the transport, the stage beside what was
//! opened, and a status line.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use cuelight_core::{LayerPath, Property};
use cuelight_editor_core::assets::Asset;
use cuelight_editor_core::document::Document;
use cuelight_editor_core::inputs::{self, Inputs};
use cuelight_editor_core::session::{Instant, lock};
use cuelight_editor_core::specimen::Sizing;
use cuelight_editor_core::tree::{self, Row};
use iced::keyboard;
use iced::widget::Widget as _;
use iced::widget::pane_grid::{self, Axis, Configuration};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{
    button, center, column, container, image, responsive, row, scrollable, slider, space, svg,
    text, toggler,
};
use iced::{Element, Fill, Font, Subscription, Task, Theme};

use crate::dialog::{self, Picked};
use crate::stage::Pick;
use cuelight_editor_core::opened::{self, Opened, Summary};
use cuelight_editor_core::save::{self, Origin};
use cuelight_editor_core::session::Session;
#[cfg(not(target_arch = "wasm32"))]
use cuelight_editor_core::watch;

mod assets;
mod editing;
mod inputs_panel;
mod inspector;
mod library;
mod sound;
mod stage;
#[cfg(not(target_arch = "wasm32"))]
mod watching;

use assets::{faces, load_fonts, thumbs};
use inputs_panel::key_name;

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
    /// Where a save writes the show, and every file it shipped as last
    /// opened or saved.
    origin: Option<Origin>,
    files: BTreeMap<String, Vec<u8>>,
    /// The show changed on disk while it had unsaved edits: what is
    /// there now, until the person says which to keep (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    outside: Option<watch::Change>,
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
    /// The scale that fits the show into the stage area, as the last
    /// layout found it: what zooming in or out starts from while fitted.
    fitted: Cell<f32>,
    /// Whether the log below the stage is unfolded, or just its header.
    log_open: bool,
    /// Whether the playhead counts from when the active scene was
    /// entered, rather than from the start of the session.
    follow_scene: bool,
    /// Where the stage is scrolled to, as the last scroll left it; `None`
    /// while it is centred on the show, which a fresh open and a fit ask
    /// for. A zoom step scales it, so the point under the middle of the
    /// view stays there.
    scrolled: Option<AbsoluteOffset>,
    /// The show document as written, for the inspector to show a
    /// layer's own text, and as it is edited.
    document: Option<Document>,
    /// What was typed into the inspector's fields, for the layer they
    /// were typed for, until it is applied.
    typed: Option<(LayerPath, Vec<(Property, String)>)>,
    /// An edit waiting for a yes, because a timeline or binding owns the
    /// property at the playhead.
    owned: Option<editing::Owned>,
    /// The base value of each editable property of the picked layer, as
    /// its field shows it. Kept here because a field borrows it.
    bases: Vec<(Property, String)>,
    /// The picked layer's properties it writes; the rest are defaults.
    written: Vec<Property>,
    /// The picked layer's other fields, as their rows show them. Kept
    /// here because a row's field borrows its text.
    layer_fields: Vec<editing::LayerField>,
    /// What was typed into those rows, for the layer it was typed for.
    field_typed: Option<(LayerPath, Vec<(&'static str, String)>)>,
    /// A number being dragged by its label.
    scrub: Option<editing::Scrub>,
    /// Whether the system asks for a light or a dark theme: iced draws
    /// the window in the theme it picks for it, and the inspector's
    /// colours are taken from the same one.
    mode: iced::theme::Mode,
}

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
    /// Write the show back where it came from; in a browser, download it.
    Save,
    /// Text typed into a property's field in the inspector.
    Type(Property, String),
    /// Enter in a property's field: set its base value to what was typed.
    Apply(Property),
    /// A toggle flipped or a name picked: set the base value to it.
    Put(Property, String),
    /// A click or Tab while something typed waits: it is applied, since
    /// that is what leaving a field looks like.
    Commit,
    /// Take a property out of the layer, back to its default.
    Reset(Property),
    /// Take a field out of the layer, back to its default.
    ResetField(&'static str),
    /// Text typed into one of the layer's other fields, by its label.
    TypeField(&'static str, String),
    /// Enter in a field's row: write what was typed.
    ApplyField(&'static str),
    /// A field's toggle flipped or word picked.
    PutField(&'static str, String),
    /// Set the base anyway, though a timeline or binding owns it now.
    EditOwned,
    /// Leave the property as it was.
    KeepOwned,
    Undo,
    Redo,
    /// The mouse went down on a property's label: a drag changes the
    /// number, a click unfolds where it comes from.
    ScrubStart(Property),
    /// The cursor's x while a label is held.
    ScrubMove(f32),
    /// A frame while dragging: the number follows the cursor.
    ScrubApply,
    ScrubEnd,
    /// Files of the show's folder changed on disk, by the editor or not.
    #[cfg(not(target_arch = "wasm32"))]
    DiskChanged(Vec<std::path::PathBuf>),
    /// The show changed on disk while it had unsaved edits: keep the
    /// edits (the next save writes over the change), or load the show
    /// from disk (the edits are dropped).
    #[cfg(not(target_arch = "wasm32"))]
    KeepEdits,
    #[cfg(not(target_arch = "wasm32"))]
    LoadFromDisk,
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
    /// A scene's heading clicked in the tree: enter that scene, paused.
    EnterScene(usize),
    /// Count the playhead from when the active scene was entered, or
    /// from the start of the session.
    FollowScene(bool),
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
            origin: None,
            files: BTreeMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            outside: None,
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
            typed: None,
            owned: None,
            bases: Vec::new(),
            written: Vec::new(),
            scrub: None,
            layer_fields: Vec::new(),
            field_typed: None,
            mode: iced::theme::Mode::None,
            fitted: Cell::new(1.0),
            log_open: true,
            follow_scene: false,
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
            Some(_) if self.document.as_ref().is_some_and(Document::is_dirty) => {
                format!("{}* - cuelight editor", self.summary.name)
            }
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
        self.refresh_bases();
        self.refresh_fields_of_layer();
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
        // Leaving a field applies it: typing into another, picking
        // another layer, saving.
        match &message {
            Message::Type(property, _) => {
                self.commit_typed(Some(editing::Typed::Property(*property)))
            }
            Message::TypeField(label, _) => self.commit_typed(Some(editing::Typed::Field(label))),
            Message::Choose(_)
            | Message::Pick(..)
            | Message::Deselect
            | Message::Save
            | Message::ScrubStart(_)
            | Message::Put(..)
            | Message::PutField(..)
            | Message::Reset(_)
            | Message::ResetField(_) => self.commit_typed(None),
            _ => {}
        }
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
            Message::Save => self.save(),
            Message::Type(property, text) => {
                self.type_into(property, text);
                Task::none()
            }
            Message::Apply(property) => self.apply(property),
            Message::Put(property, value) => self.put(property, value),
            Message::Commit => {
                self.commit_typed(None);
                Task::none()
            }
            Message::Reset(property) => self.reset(property),
            Message::ResetField(label) => self.reset_field(label),
            Message::TypeField(label, text) => {
                self.type_into_field(label, text);
                Task::none()
            }
            Message::ApplyField(label) => self.apply_field(label),
            Message::PutField(label, value) => self.put_field(label, value),
            Message::EditOwned => self.edit_owned(),
            Message::KeepOwned => {
                self.owned = None;
                Task::none()
            }
            Message::ScrubStart(property) => self.scrub_start(property),
            Message::ScrubMove(x) => self.scrub_move(x),
            Message::ScrubApply => self.scrub_apply(),
            Message::ScrubEnd => self.scrub_end(),
            Message::Undo => self.undo(false),
            Message::Redo => self.undo(true),
            #[cfg(not(target_arch = "wasm32"))]
            Message::DiskChanged(paths) => self.disk_changed(&paths),
            #[cfg(not(target_arch = "wasm32"))]
            Message::KeepEdits => {
                self.outside = None;
                self.status = "kept your edits; saving writes over the show on disk".to_owned();
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::LoadFromDisk => match self.outside.take() {
                Some(change) => self.reload(change),
                None => Task::none(),
            },
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
            Message::EnterScene(scene) => {
                if let Some(session) = &mut self.session {
                    session.enter_scene(scene, Instant::now());
                    self.follow_scene = true;
                    self.hush();
                }
                Task::none()
            }
            Message::FollowScene(on) => {
                self.follow_scene = on;
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
                    "s" if modifiers.control() => self.update(Message::Save),
                    "z" | "Z" if modifiers.control() && modifiers.shift() => {
                        self.update(Message::Redo)
                    }
                    "z" if modifiers.control() => self.update(Message::Undo),
                    "y" if modifiers.control() => self.update(Message::Redo),
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
                            let engine = lock(&session.engine);
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
                let under = lock(&session.engine).layers_at(point);
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

    /// Load `text`, the document as it stands, into the playing session
    /// at the playhead, and bring what the window shows of the show up to
    /// date. Text that does not load leaves the show as it was.
    /// The audit can take longer than a frame: a drag leaves it out
    /// until it ends.
    fn reload_text(&mut self, text: &str, audit: bool) -> Result<(), String> {
        let Some(session) = &mut self.session else {
            return Ok(());
        };
        let findings = session
            .reload(text, Instant::now())
            .map_err(|e| format!("the show does not load ({e}); still showing the last version"))?;
        let findings: Vec<String> = findings.iter().map(ToString::to_string).collect();
        session
            .log
            .extend(cuelight_editor_core::log::load(&findings));
        if audit {
            session.audit(text);
        }
        let engine = lock(&session.engine);
        if let Some(show) = engine.show() {
            self.rows = tree::rows(show);
            self.inputs = Inputs::of(show);
        }
        self.summary = opened::resummarize(&engine, &self.summary, &findings);
        drop(engine);
        // What was picked stays picked, where the show still has it.
        let rows = &self.rows;
        self.selection.retain(|picked| {
            rows.iter()
                .any(|row| matches!(row, Row::Layer { path, .. } if path == picked))
        });
        Ok(())
    }

    /// Save the open show where it came from; a browser downloads it.
    fn save(&mut self) -> Task<Message> {
        let (Some(origin), Some(document)) = (&self.origin, &mut self.document) else {
            return Task::none();
        };
        #[cfg(not(target_arch = "wasm32"))]
        let result = save::save(origin, &mut self.files, document)
            .map(|()| format!("saved {}", self.source));
        #[cfg(target_arch = "wasm32")]
        let result = save::download(origin, &self.files, document).and_then(|(name, bytes)| {
            dialog::offer_download(&name, &bytes).map_err(save::SaveError::Write)?;
            document.mark_saved();
            Ok(format!("downloaded {name}"))
        });
        match result {
            Ok(said) => {
                self.status = said;
                log::info!("{}", self.status);
            }
            Err(error) => {
                self.status = format!("could not save: {error}");
                log::warn!("{}", self.status);
            }
        }
        Task::none()
    }

    fn open(&mut self, result: Result<Opened, opened::OpenError>) -> Task<Message> {
        match result {
            Ok(opened) => {
                self.status = format!("opened {}", opened.source);
                log::info!("{}", self.status);
                let Opened {
                    source,
                    origin,
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
                self.origin = Some(origin);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.outside = None;
                }
                self.summary = summary;
                self.thumbs = thumbs(&engine, &library);
                self.faces = faces(&engine, &files, &library);
                self.loaded.clear();
                let fonts = load_fonts(&engine, &library);
                self.library = library;
                self.preview = None;
                self.selected = None;
                self.rows = engine.show().map(tree::rows).unwrap_or_default();
                self.follow_scene = false;
                self.selection.clear();
                self.expanded = None;
                self.typed = None;
                self.owned = None;
                self.scrub = None;
                self.field_typed = None;
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
                self.files = files;
                self.document = Some(document);
                self.session = Some(session);
                let centred = self.centre_stage();
                Task::batch([task, fonts, centred])
            }
            Err(error) => {
                self.status = format!("could not open: {error}");
                log::warn!("{}", self.status);
                Task::none()
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let keys = keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed { key, modifiers, .. } => {
                Some(Message::KeyPressed(key, modifiers))
            }
            _ => None,
        });
        let mut subscriptions = vec![keys, iced::system::theme_changes().map(Message::Mode)];
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
        // Typing waits to be applied: a click anywhere, or Tab, is the
        // field being left, which a field itself does not say.
        if self.has_typed() {
            subscriptions.push(iced::event::listen_with(
                |event, _status, _window| match event {
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => {
                        Some(Message::Commit)
                    }
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                        ..
                    }) => Some(Message::Commit),
                    _ => None,
                },
            ));
        }
        // A label held: the mouse anywhere moves its number, until it
        // comes up.
        if self.scrub.as_ref().is_some_and(|s| s.dragging) {
            subscriptions.push(iced::window::frames().map(|_| Message::ScrubApply));
        }
        if self.scrub.is_some() {
            subscriptions.push(iced::event::listen_with(
                |event, _status, _window| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::ScrubMove(position.x))
                    }
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced::mouse::Button::Left,
                    )) => Some(Message::ScrubEnd),
                    _ => None,
                },
            ));
        }
        // The open show's files on disk, for changes made outside.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((at, under)) = self.origin.as_ref().and_then(watch::watched) {
            subscriptions.push(
                Subscription::run_with((at.to_owned(), under), crate::watcher::watch)
                    .map(Message::DiskChanged),
            );
        }
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
                    .on_press_maybe((!self.asking).then_some(Message::OpenFolder))
                    .boxed(),
            );
        }
        let dirty = self.document.as_ref().is_some_and(Document::is_dirty);
        bar = bar.push(
            button("Save")
                .on_press_maybe(dirty.then_some(Message::Save))
                .boxed(),
        );
        if let Some(session) = &self.session {
            // The playhead covers one pass of the driver, or as far as
            // the show has played, whichever is longer. Following the
            // scene, it counts from when the scene was entered, to the
            // end of its longest timeline.
            let scenes = self.summary.scenes > 0;
            let follow = self.follow_scene && scenes;
            let from = if follow { session.entered } else { 0.0 };
            let time = session.time - from;
            let end = if follow {
                session.scene_length()
            } else {
                session.pass_length().unwrap_or(60.0)
            }
            .max(time)
            .max(1.0);
            bar = bar
                .push(space::horizontal().width(16).boxed())
                .push(button("|<").on_press(Message::Restart).boxed())
                .push(button("<").on_press(Message::Step(-1.0 / 60.0)).boxed())
                .push(
                    button(if session.paused { "Play" } else { "Pause" })
                        .on_press(Message::TogglePause)
                        .boxed(),
                )
                .push(button(">").on_press(Message::Step(1.0 / 60.0)).boxed())
                .push(
                    slider(0.0..=end, time, move |t| Message::Seek(from + t))
                        .step(1.0 / 60.0)
                        .width(Fill)
                        .boxed(),
                )
                .push(text(format!("{time:7.2} / {end:.0} s")).size(14).boxed())
                .push(space::horizontal().width(16).boxed());
            if scenes {
                bar = bar.push(
                    toggler(follow)
                        .label("Scene clock")
                        .on_toggle(Message::FollowScene)
                        .size(16)
                        .boxed(),
                );
            }
            if session.has_driver() {
                bar = bar.push(
                    toggler(session.driving)
                        .label("Driver")
                        .on_toggle(Message::Drive)
                        .size(16)
                        .boxed(),
                );
            }
            bar = bar
                .push(space::horizontal().width(16).boxed())
                .push(text(&self.source).size(14).boxed());
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
            .boxed(),
            Some(session) => iced::widget::pane_grid(&self.panes, move |_, pane, _| {
                pane_grid::Content::new(match pane {
                    Pane::Inputs => scrollable(self.inputs_panel(session))
                        .width(Fill)
                        .height(Fill)
                        .boxed(),
                    Pane::Stage => column![
                        responsive(move |size| self.stage(session, size)),
                        self.log_panel(session),
                    ]
                    .boxed(),
                    Pane::Library => self.library_panel(session),
                    // The preview of an asset fits the pane, so the pane
                    // says how large it is.
                    Pane::Inspector => responsive(move |size| {
                        scrollable(self.inspector_panel(session, size))
                            .width(Fill)
                            .height(Fill)
                    })
                    .boxed(),
                })
            })
            .on_resize(8, Message::Resized)
            .spacing(4)
            .min_size(120)
            .width(Fill)
            .height(Fill)
            .boxed(),
        };

        let status = container(text(&self.status).size(13))
            .padding([4, 8])
            .width(Fill);

        // A change on disk that would drop unsaved edits waits for a say.
        #[cfg(not(target_arch = "wasm32"))]
        let asking = self.outside_prompt();
        #[cfg(target_arch = "wasm32")]
        let asking: Option<Element<'_, Message>> = None;
        column![container(bar).padding(8).width(Fill), asking, body, status].boxed()
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
mod tests;
