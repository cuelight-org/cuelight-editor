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
    button, center, column, container, image, pick_list, responsive, row, scrollable, slider,
    space, svg, text, toggler, tooltip,
};
use iced::{Element, Fill, Font, Subscription, Task, Theme};

use crate::dialog::{self, Picked};
use crate::stage::{Grip, Held, Pick};
use cuelight_editor_core::opened::{self, Opened, Summary};
use cuelight_editor_core::save::{self, Origin};
use cuelight_editor_core::session::Session;
#[cfg(not(target_arch = "wasm32"))]
use cuelight_editor_core::watch;

mod arrange;
mod assets;
mod editing;
mod inputs_panel;
mod inspector;
mod journal;
mod library;
mod lists;
mod manipulate;
mod scenes;
mod solo;
mod sound;
mod stage;
mod styles;
#[cfg(not(target_arch = "wasm32"))]
mod watching;

use assets::{faces, load_fonts, thumbs};
use inputs_panel::key_name;
use solo::SoloMessage;

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
    /// Solo the layer picked with `--pick`, after the triggers.
    #[arg(long)]
    pub solo: bool,
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

/// What was typed into the field rows, label and text, and what the
/// rows were of.
type TypedFields = (editing::FieldsOf, Vec<(&'static str, String)>);

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
    /// The font style picked in the assets, by name: its fields in the
    /// inspector.
    style: Option<String>,
    /// A new name typed for the picked font style, not applied yet.
    style_name_typed: Option<String>,
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
    /// The scene whose heading was picked in the tree, while no layer
    /// is: its settings in the inspector.
    scene: Option<usize>,
    /// How far down the tree is scrolled, and how tall its view is.
    tree_view: (f32, f32),
    /// The tree's headings and groups folded, by their keys.
    folded: BTreeSet<String>,
    /// The scene active when the tree last looked: a scene that becomes
    /// active unfolds.
    active_seen: Option<String>,
    /// What was typed into the picked scene's trigger rows, by row
    /// (`None` for the row that adds one).
    triggers_typed: BTreeMap<Option<usize>, String>,
    /// The inspector row unfolded to list every source of its value.
    expanded: Option<Property>,
    /// The colour field whose channels are unfolded, by label.
    unfolded_field: Option<&'static str>,
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
    /// The picked layer's JSON as it was when a drag began: the inspector
    /// shows it until the drag lets go, rather than laying the text out
    /// again on every frame.
    held_json: std::cell::RefCell<Option<String>>,
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
    /// The open show's unsaved edits, kept against a crash.
    journal: journal::Journal,
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
    /// The picked layer's other fields, or the show's settings while
    /// nothing is picked, as their rows show them. Kept here because a
    /// row's field borrows its text.
    layer_fields: Vec<editing::LayerField>,
    /// What was typed into those rows.
    field_typed: Option<TypedFields>,
    /// What was typed into the rows of the show's lists.
    list_typed: BTreeMap<lists::Row, lists::Draft>,
    /// A number being dragged by its label.
    scrub: Option<editing::Scrub>,
    /// The selection being dragged on the stage, or one of its handles.
    grab: Option<manipulate::Grab>,
    /// Whether the system asks for a light or a dark theme: iced draws
    /// the window in the theme it picks for it, and the inspector's
    /// colours are taken from the same one.
    mode: iced::theme::Mode,
    /// Light or dark as picked in the top bar, over the system's
    /// preference; `None` follows the system.
    theme_pick: Option<iced::theme::Mode>,
    /// The log as the panel under the stage shows it: text to select and
    /// copy from.
    log_view: iced::widget::text_editor::Content,
    /// The log's revision the view was made from.
    log_seen: Option<u64>,
    /// The log's top line in view, as its own scrolling has left it.
    log_top: f32,
    /// The window was asked to close with unsaved edits: what to do with
    /// them waits for an answer.
    closing: bool,
    /// The SVG path data typed for a path the add menu makes, while it
    /// waits for it.
    path_typed: Option<String>,
    /// The modifier keys held: Shift or Ctrl with a click in the tree
    /// adds to the selection.
    held: keyboard::Modifiers,
    /// The layer soloed, playing alone on the stage.
    solo: Option<cuelight_editor_core::solo::Solo>,
    /// How many frames a strip of the solo takes, and over how many
    /// seconds, as typed.
    strip_frames: String,
    strip_seconds: String,
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
    /// A drag on the stage began: the selection, or one of its handles,
    /// taken hold of at a canvas point.
    Grab(Grip, [f64; 2]),
    /// Where the drag is on the canvas, and the keys held.
    Drag([f64; 2], Held),
    /// A frame while dragging: the selection follows the pointer.
    DragApply,
    /// The drag let go.
    Release,
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
    /// The opened show's journal had unsaved edits: put them back as
    /// one step to undo, or drop them.
    RestoreEdits,
    DiscardEdits,
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
    /// A scene's heading clicked in the tree: its settings in the
    /// inspector, and the scene entered, paused.
    PickScene(usize),
    /// A scene picked in the top bar's menu: enter it, paused.
    EnterScene(usize),
    /// A new scene after the picked one, picked.
    /// The tree scrolled: how far down, and how tall its view is.
    TreeScrolled(f32, f32),
    /// A heading or group folded or unfolded, by its key.
    ToggleFold(String),
    AddScene,
    /// Take the picked scene out, with its layers.
    DeleteScene,
    /// Move the picked scene one place up (`true`) or down.
    MoveScene(bool),
    /// A trigger of the picked scene typed into, by row (`None` for the
    /// row that adds one); Enter in one, which writes all typed; or a
    /// row taken out.
    TypeTrigger(Option<usize>, String),
    ApplyTriggers,
    RemoveTrigger(usize),
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
    /// A layer picked in the tree; with Shift or Ctrl held, added to
    /// the selection or taken out of it.
    Choose(LayerPath),
    /// The modifier keys held changed.
    Modifiers(keyboard::Modifiers),
    /// The add menu picked a kind of layer.
    AddLayer(cuelight_editor_core::layers::Kind),
    /// SVG path data typed for a new path, then added, or not.
    TypePath(String),
    AddPath,
    CancelPath,
    /// Take the picked layers out (Delete).
    DeleteLayers,
    /// Copy the picked layers, each right after itself (Ctrl+D).
    DuplicateLayers,
    /// Move the picked layers one place up the tree (`true`) or down
    /// (Alt+Up, Alt+Down).
    Reorder(bool),
    /// Put the picked layers into a new group (Ctrl+G).
    GroupLayers,
    /// Put the picked group's children where it is (Ctrl+Shift+G).
    Ungroup,
    /// Move the picked layers to the end of a group, a scene or the
    /// show's layers.
    MoveLayers(arrange::Destination),
    /// The selection cleared.
    Deselect,
    /// An inspector row unfolded, or all folded.
    Expand(Option<Property>),
    /// A colour field's channels unfolded, or folded with `None`.
    UnfoldField(Option<&'static str>),
    /// A name or a value typed into a row of one of the show's lists,
    /// by the name the row had (`None` for the row that adds one).
    TypeListName(cuelight_editor_core::lists::List, Option<String>, String),
    TypeListValue(cuelight_editor_core::lists::List, Option<String>, String),
    /// Enter in a list's row.
    ApplyList(cuelight_editor_core::lists::List, Option<String>),
    RemoveListRow(cuelight_editor_core::lists::List, String),
    /// The log below the stage folded to its header, or unfolded.
    ToggleLog,
    /// A split between two areas dragged.
    Resized(pane_grid::ResizeEvent),
    /// The system's light or dark preference, found or changed.
    Mode(iced::theme::Mode),
    /// A font style picked in the assets, by name.
    PickStyle(String),
    /// A new font style in this font file.
    AddStyle(String),
    /// A new name typed for the picked font style, and Enter in it.
    TypeStyleName(String),
    ApplyStyleName,
    /// Take the picked font style out, which nothing uses.
    RemoveStyle,
    /// Light or dark picked in the top bar, or back to the system's.
    PickTheme(Option<iced::theme::Mode>),
    /// A selection or a move in the log; edits are not let through.
    LogAction(iced::widget::text_editor::Action),
    /// The whole log to the clipboard.
    CopyLog,
    /// The bar beside the log moved it to this top line.
    LogScrollTo(f32),
    /// The window's close button, or the system asking it to close.
    CloseRequested,
    /// The answers to closing with unsaved edits.
    SaveAndClose,
    CloseWithoutSaving,
    KeepOpen,
    /// The window as drawn, for `--screenshot`.
    #[cfg(not(target_arch = "wasm32"))]
    Shot(iced::window::Screenshot),
    /// The browser decoded the show's sounds: each name with its length,
    /// or why it did not decode.
    #[cfg(target_arch = "wasm32")]
    SoundsReady(Vec<(String, Result<f64, String>)>),
    /// Something done to the solo, or soloing.
    Solo(SoloMessage),
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
            style: None,
            style_name_typed: None,
            actual_size: false,
            tab: Tab::Layers,
            rows: Vec::new(),
            selection: Vec::new(),
            scene: None,
            tree_view: (0.0, 0.0),
            folded: BTreeSet::new(),
            active_seen: None,
            triggers_typed: BTreeMap::new(),
            expanded: None,
            unfolded_field: None,
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
            journal: journal::Journal::new(),
            typed: None,
            owned: None,
            bases: Vec::new(),
            written: Vec::new(),
            scrub: None,
            grab: None,
            layer_fields: Vec::new(),
            field_typed: None,
            list_typed: BTreeMap::new(),
            mode: iced::theme::Mode::None,
            theme_pick: None,
            log_view: iced::widget::text_editor::Content::new(),
            log_seen: None,
            log_top: 0.0,
            closing: false,
            path_typed: None,
            held: keyboard::Modifiers::default(),
            solo: None,
            strip_frames: "8".to_owned(),
            strip_seconds: "1".to_owned(),
            fitted: Cell::new(1.0),
            held_json: std::cell::RefCell::new(None),
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
            if options.solo {
                then = then.chain(Task::done(Message::Solo(SoloMessage::Enter)));
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
        // The pointer moving in a drag only says where it is; the next
        // frame writes it. Nothing else has changed to refresh, nor on a
        // frame that waits to write.
        if let Message::Drag(at, held) = message {
            return self.drag(at, held);
        }
        if matches!(message, Message::DragApply) && self.drag_waits() {
            return Task::none();
        }
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
        // What adds to the tree or moves in it brings what it picked
        // into the tree's view.
        let reveal = matches!(
            message,
            Message::AddLayer(_)
                | Message::AddPath
                | Message::DuplicateLayers
                | Message::Reorder(_)
                | Message::GroupLayers
                | Message::Ungroup
                | Message::MoveLayers(_)
                | Message::AddScene
                | Message::MoveScene(_)
        );
        let picked_on_stage = matches!(message, Message::Pick(..));
        let picked_in_tree = matches!(message, Message::Choose(_));
        let questions = self.questions();
        let task = self.handle(message);
        // A layer picked in the tree, or added, shows on the stage: its
        // scene is entered.
        let task = if reveal || picked_in_tree {
            Task::batch([task, self.enter_picked_scene()])
        } else {
            task
        };
        // A layer picked on the stage, or one just added or moved, is
        // never left inside something folded.
        if reveal || picked_on_stage {
            self.unfold_to_picked();
        }
        let active = self.session.as_ref().and_then(Session::active_scene);
        if active != self.active_seen {
            if let Some(name) = &active {
                self.folded.remove(&format!("scene {name}"));
            }
            self.active_seen = active;
        }
        let task = if reveal {
            Task::batch([task, self.reveal_in_tree()])
        } else {
            task
        };
        // A question coming or going changes the stage's room: a fitted
        // show is centred again in it. At a set zoom the scroll keeps the
        // same point in the middle by itself.
        let task = if self.questions() != questions {
            Task::batch([task, self.recentre_fitted()])
        } else {
            task
        };
        self.keep_journal();
        self.refresh_fields();
        self.refresh_bases();
        self.refresh_fields_of_layer();
        self.refresh_log();
        task
    }

    /// How many bars stand above the stage: the questions waiting, and
    /// the solo's.
    fn questions(&self) -> usize {
        #[cfg(not(target_arch = "wasm32"))]
        let outside = self.outside.is_some();
        #[cfg(target_arch = "wasm32")]
        let outside = false;
        [
            self.closing,
            outside,
            self.journal.offer.is_some(),
            self.solo.is_some(),
        ]
        .into_iter()
        .filter(|&asked| asked)
        .count()
    }

    /// The session the host's inputs go to: the solo's while soloing,
    /// the show's otherwise.
    fn played(&mut self) -> Option<&mut Session> {
        match &mut self.solo {
            Some(solo) => Some(&mut solo.session),
            None => self.session.as_mut(),
        }
    }

    /// What the variable fields show now.
    fn refresh_fields(&mut self) {
        let Some(session) = self
            .solo
            .as_ref()
            .map(|s| &s.session)
            .or(self.session.as_ref())
        else {
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
            Message::TypeListName(list, was, _) | Message::TypeListValue(list, was, _) => {
                self.commit_typed(Some(editing::Typed::Row(*list, was.clone())))
            }
            Message::TypeTrigger(row, _) => self.commit_typed(Some(editing::Typed::Trigger(*row))),
            Message::Choose(_)
            | Message::Pick(..)
            | Message::Deselect
            | Message::Save
            | Message::ScrubStart(_)
            | Message::Grab(..)
            | Message::Put(..)
            | Message::PutField(..)
            | Message::RemoveListRow(..)
            | Message::PickStyle(_)
            | Message::Select(_)
            | Message::CloseRequested
            | Message::Reset(_)
            | Message::ResetField(_)
            | Message::AddLayer(_)
            | Message::AddPath
            | Message::DeleteLayers
            | Message::DuplicateLayers
            | Message::Reorder(_)
            | Message::GroupLayers
            | Message::Ungroup
            | Message::MoveLayers(_)
            | Message::PickScene(_)
            | Message::AddScene
            | Message::DeleteScene
            | Message::MoveScene(_)
            | Message::RemoveTrigger(_) => self.commit_typed(None),
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
            Message::TypeListName(list, was, name) => {
                self.type_list((list, was), Some(name), None);
                Task::none()
            }
            Message::TypeListValue(list, was, value) => {
                self.type_list((list, was), None, Some(value));
                Task::none()
            }
            Message::ApplyList(list, was) => self.apply_list((list, was)),
            Message::RemoveListRow(list, name) => self.remove_list_row(list, name),
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
            Message::Grab(grip, at) => self.grab(grip, at),
            Message::Drag(at, held) => self.drag(at, held),
            Message::DragApply => self.drag_apply(),
            Message::Release => self.release(),
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
                Some(change) => {
                    self.forget_edits();
                    self.reload(change)
                }
                None => Task::none(),
            },
            Message::RestoreEdits => self.restore_edits(),
            Message::DiscardEdits => self.discard_edits(),
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
                if let Some(solo) = &mut self.solo
                    && !solo.session.paused
                {
                    solo.session.tick(now);
                }
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
            Message::PickScene(scene) => self.pick_scene(scene),
            Message::EnterScene(scene) => self.enter_scene(scene),
            Message::AddScene => self.add_scene(),
            Message::ToggleFold(key) => {
                if !self.folded.remove(&key) {
                    self.folded.insert(key);
                }
                Task::none()
            }
            Message::TreeScrolled(offset, height) => {
                self.tree_view = (offset, height);
                Task::none()
            }
            Message::DeleteScene => self.delete_scene(),
            Message::MoveScene(up) => self.move_scene(up),
            Message::TypeTrigger(row, typed) => {
                self.type_trigger(row, typed);
                Task::none()
            }
            // Enter applies the rows typed into, as leaving them does.
            Message::ApplyTriggers => self.apply_triggers(None),
            Message::RemoveTrigger(row) => self.remove_trigger(row),
            Message::FollowScene(on) => {
                self.follow_scene = on;
                Task::none()
            }
            Message::KeyPressed(key, modifiers) => {
                let name = key_name(&key);
                // Alt with an arrow moves the picked layers in the tree,
                // or the picked scene.
                if modifiers.alt() && !self.selection.is_empty() {
                    match name.as_str() {
                        "ArrowUp" => return self.update(Message::Reorder(true)),
                        "ArrowDown" => return self.update(Message::Reorder(false)),
                        _ => {}
                    }
                }
                if modifiers.alt() && self.selection.is_empty() && self.scene.is_some() {
                    match name.as_str() {
                        "ArrowUp" => return self.update(Message::MoveScene(true)),
                        "ArrowDown" => return self.update(Message::MoveScene(false)),
                        _ => {}
                    }
                }
                // With a layer picked the arrows nudge it, before the show
                // hears them.
                if !modifiers.control()
                    && !self.selection.is_empty()
                    && let Some(by) = manipulate::nudge_by(&name, modifiers.shift())
                {
                    return self.nudge(by);
                }
                // Soloing, the transport keys and the show's own keys are
                // the solo's, and Escape leaves it.
                if !modifiers.control()
                    && let Some(solo) = &mut self.solo
                {
                    match name.as_str() {
                        " " => return self.update(Message::Solo(SoloMessage::TogglePause)),
                        "r" => return self.update(Message::Solo(SoloMessage::Restart)),
                        "Escape" => return self.update(Message::Solo(SoloMessage::Leave)),
                        _ if self.inputs.keys.contains_key(name.as_str()) => {
                            solo.session.key(&name);
                            return Task::none();
                        }
                        _ => {}
                    }
                }
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
                    "d" | "D" if modifiers.control() => self.update(Message::DuplicateLayers),
                    "g" | "G" if modifiers.control() && modifiers.shift() => {
                        self.update(Message::Ungroup)
                    }
                    "g" if modifiers.control() => self.update(Message::GroupLayers),
                    "Delete" if self.selection.is_empty() && self.scene.is_some() => {
                        self.update(Message::DeleteScene)
                    }
                    "Delete" => self.update(Message::DeleteLayers),
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
                if let Some(session) = self.played() {
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
                if let Some(session) = self.played() {
                    session.set(&name, inputs::parse_value(&text));
                }
                Task::none()
            }
            Message::Record(on) => {
                if let Some(session) = self.played() {
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
                self.style = None;
                Task::none()
            }
            Message::PickStyle(name) => {
                self.tab = Tab::Assets;
                self.selected = None;
                self.style = Some(name);
                Task::none()
            }
            Message::AddStyle(font) => self.add_style(font),
            Message::TypeStyleName(name) => {
                self.style_name_typed = Some(name);
                Task::none()
            }
            Message::ApplyStyleName => self.rename_style(),
            Message::RemoveStyle => self.remove_style(),
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
                let under = self.layers_under(&lock(&session.engine), point);
                self.pick(under, pick);
                if !self.selection.is_empty() {
                    self.scene = None;
                    self.triggers_typed.clear();
                }
                // A layer picked is what the inspector shows now.
                self.selected = None;
                Task::none()
            }
            Message::Choose(path) => {
                // Also from the inspector's list of an asset's uses: the
                // layer is then shown where the tree has it.
                self.tab = Tab::Layers;
                if self.held.shift() || self.held.control() {
                    match self.selection.iter().position(|p| *p == path) {
                        Some(i) => {
                            self.selection.remove(i);
                        }
                        None => self.selection.push(path),
                    }
                } else {
                    self.selection = vec![path];
                }
                self.scene = None;
                self.triggers_typed.clear();
                self.selected = None;
                self.expanded = None;
                self.unfolded_field = None;
                Task::none()
            }
            Message::Modifiers(held) => {
                self.held = held;
                Task::none()
            }
            Message::AddLayer(kind) => self.add_layer(kind),
            Message::TypePath(path) => {
                self.path_typed = Some(path);
                Task::none()
            }
            Message::AddPath => self.add_path(),
            Message::CancelPath => {
                self.path_typed = None;
                Task::none()
            }
            Message::DeleteLayers => self.delete_layers(),
            Message::DuplicateLayers => self.duplicate_layers(),
            Message::Reorder(up) => self.reorder(up),
            Message::GroupLayers => self.group_layers(),
            Message::Ungroup => self.ungroup(),
            Message::MoveLayers(to) => self.move_layers(to),
            Message::Deselect => {
                self.selection.clear();
                self.scene = None;
                self.triggers_typed.clear();
                self.selected = None;
                self.expanded = None;
                self.unfolded_field = None;
                Task::none()
            }
            Message::Expand(property) => {
                self.expanded = property;
                Task::none()
            }
            Message::UnfoldField(label) => {
                self.unfolded_field = label;
                Task::none()
            }
            Message::CloseRequested => {
                if self.document.as_ref().is_some_and(Document::is_dirty) {
                    self.closing = true;
                    Task::none()
                } else {
                    iced::exit()
                }
            }
            Message::SaveAndClose => {
                let task = self.save();
                if self.document.as_ref().is_some_and(Document::is_dirty) {
                    // Not saved: the status says why, and the window stays.
                    self.closing = false;
                    task
                } else {
                    iced::exit()
                }
            }
            // The journal keeps the edits: the next open offers them back.
            Message::CloseWithoutSaving => iced::exit(),
            Message::KeepOpen => {
                self.closing = false;
                Task::none()
            }
            Message::LogAction(action) => {
                use iced::widget::text_editor::Action;
                match action {
                    _ if action.is_edit() => {}
                    // A drag past the bottom or the top scrolls on, so a
                    // selection can run past what the log shows.
                    Action::Drag(at) => {
                        // Measured inside the editor's padding, 5 above
                        // and 5 below.
                        if at.y > stage::LOG_HEIGHT - 10.0 {
                            self.scroll_log(1);
                        } else if at.y < 0.0 {
                            self.scroll_log(-1);
                        }
                        self.log_view.perform(Action::Drag(at));
                    }
                    Action::Scroll { lines } => self.scroll_log(lines),
                    action => self.log_view.perform(action),
                }
                Task::none()
            }
            Message::LogScrollTo(top) => {
                let lines = (top - self.log_top).round() as i32;
                self.scroll_log(lines);
                Task::none()
            }
            Message::CopyLog => {
                self.status = "copied the log".to_owned();
                iced::clipboard::write(self.log_view.text()).discard()
            }
            Message::PickTheme(pick) => {
                self.theme_pick = pick;
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
            Message::Solo(message) => self.solo_message(message),
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
        if let Some(scene) = self.scene
            && !self
                .rows
                .iter()
                .any(|row| matches!(row, Row::Root { root: cuelight_core::Root::Scene(i), .. } if *i == scene))
        {
            self.scene = None;
            self.triggers_typed.clear();
        }
        let rows = &self.rows;
        self.selection.retain(|picked| {
            rows.iter()
                .any(|row| matches!(row, Row::Layer { path, .. } if path == picked))
        });
        self.reload_solo();
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
                // A show opens with only what is on screen unfolded.
                self.folded = self
                    .rows
                    .iter()
                    .filter_map(|row| match row {
                        Row::Root {
                            root: cuelight_core::Root::Scene(_),
                            name,
                        } if engine.active_scene() != Some(name.as_str()) => {
                            Some(format!("scene {name}"))
                        }
                        _ => None,
                    })
                    .collect();
                self.active_seen = engine.active_scene().map(str::to_owned);
                self.follow_scene = false;
                self.solo = None;
                self.selection.clear();
                self.scene = None;
                self.triggers_typed.clear();
                self.expanded = None;
                self.unfolded_field = None;
                self.typed = None;
                self.owned = None;
                self.scrub = None;
                self.grab = None;
                self.field_typed = None;
                self.list_typed.clear();
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
                self.journal_opened();
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
            keyboard::Event::ModifiersChanged(held) => Some(Message::Modifiers(held)),
            _ => None,
        });
        let mut subscriptions = vec![
            keys,
            iced::system::theme_changes().map(Message::Mode),
            iced::window::close_requests().map(|_| Message::CloseRequested),
        ];
        #[cfg(not(target_arch = "wasm32"))]
        let waiting_to_shoot = self.session.is_some() && self.screenshot.is_some();
        #[cfg(target_arch = "wasm32")]
        let waiting_to_shoot = false;
        // Frames while the show plays, and while a previewed sound does:
        // its end is noticed on a frame.
        if self.session.as_ref().is_some_and(|s| !s.paused)
            || self.solo.as_ref().is_some_and(|s| !s.session.paused)
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
        // The same for a drag on the stage.
        if self.grab.is_some() {
            subscriptions.push(iced::window::frames().map(|_| Message::DragApply));
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
                // The scenes, the active one shown: picking one enters it.
                let active = session.active_scene();
                let choices = self.scene_choices();
                let shown = choices
                    .iter()
                    .find(|c| active.as_deref() == Some(c.name.as_str()))
                    .cloned();
                bar = bar.push(
                    pick_list(shown, choices, scenes::SceneChoice::to_string)
                        .on_select(|choice: scenes::SceneChoice| Message::EnterScene(choice.index))
                        .placeholder("Scene")
                        .text_size(13)
                        .boxed(),
                );
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
            bar = bar.push(space::horizontal().width(16).boxed()).push(
                tooltip(
                    text(short_source(&self.source))
                        .size(14)
                        .wrapping(text::Wrapping::None),
                    container(text(&self.source).size(12))
                        .padding(6)
                        .style(container::bordered_box),
                    tooltip::Position::Bottom,
                )
                .boxed(),
            );
        }
        // Light or dark, to see the editor and a show's colours in both
        // whatever the system prefers.
        let picks = [
            ThemePick(None),
            ThemePick(Some(iced::theme::Mode::Light)),
            ThemePick(Some(iced::theme::Mode::Dark)),
        ];
        bar = bar.push(space::horizontal().boxed()).push(
            pick_list(
                Some(ThemePick(self.theme_pick)),
                picks,
                ThemePick::to_string,
            )
            .on_select(|pick: ThemePick| Message::PickTheme(pick.0))
            .text_size(13)
            .boxed(),
        );

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
                    Pane::Inputs => scrollable(
                        self.inputs_panel(self.solo.as_ref().map_or(session, |s| &s.session)),
                    )
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
        // So does a journal with edits the show was not saved with.
        let restore = self.restore_prompt();
        // And closing with unsaved edits.
        let closing = self.closing.then(|| {
            question(
                "The show has unsaved edits.".to_owned(),
                [
                    ("Save", Message::SaveAndClose),
                    ("Don't save", Message::CloseWithoutSaving),
                    ("Cancel", Message::KeepOpen),
                ],
            )
        });
        // The questions sit in a column of their own, always there: one
        // coming or going leaves the body in its place, so iced keeps its
        // state (the stage's scroll, its view) rather than building it anew.
        let questions = column![closing, asking, restore];
        column![
            container(bar).padding(8).width(Fill),
            questions,
            body,
            status
        ]
        .boxed()
    }
}

/// A question the editor waits on, in a bar under the toolbar: what it
/// asks, then its answers, the first the one it suggests. In the
/// theme's warning colour, faint behind the words and full round them:
/// it asks for a decision without shouting over the stage, on a light
/// theme or a dark one.
fn question<'a, const N: usize>(
    said: String,
    answers: [(&'a str, Message); N],
) -> Element<'a, Message> {
    let mut line = row![text(said).size(13).width(Fill)]
        .spacing(8)
        .align_y(iced::Center);
    for (i, (label, message)) in answers.into_iter().enumerate() {
        let mut answer = button(text(label).size(13)).on_press(message);
        if i > 0 {
            answer = answer.style(button::secondary);
        }
        line = line.push(answer.boxed());
    }
    let bar = container(line)
        .padding([6, 10])
        .width(Fill)
        .style(|theme: &Theme| {
            let warning = theme.palette().warning.base.color;
            container::Style {
                background: Some(iced::Background::Color(warning.scale_alpha(0.18))),
                border: iced::Border {
                    color: warning,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            }
        });
    container(bar).padding([4, 8]).width(Fill).boxed()
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

/// The theme iced draws the window in: the one picked in the top bar,
/// or the one it picks for the system's preference (or `ICED_THEME`),
/// as the colours of the view that are not a widget's own style follow
/// it.
pub fn theme(app: &App) -> Theme {
    <Theme as iced::theme::Base>::default(app.theme_pick.unwrap_or(app.mode))
}

/// The theme the window is told to use: only one picked in the top bar.
/// Following the system is left to iced, which knows the preference
/// before the first frame, so a dark desktop never sees a light one.
pub fn window_theme(app: &App) -> Option<Theme> {
    app.theme_pick.map(<Theme as iced::theme::Base>::default)
}

/// A choice in the top bar's theme list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThemePick(Option<iced::theme::Mode>);

impl std::fmt::Display for ThemePick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            Some(iced::theme::Mode::Light) => "Light",
            Some(iced::theme::Mode::Dark) => "Dark",
            _ => "System theme",
        })
    }
}

/// Ask for the system's light or dark preference.
fn system_mode() -> Task<Message> {
    iced::system::theme().map(Message::Mode)
}

/// The open show's name as the top bar gives it: its folder or file,
/// with the folder for a loose `show.json`; the whole path is in the
/// bar's tooltip and the window's status line.
fn short_source(source: &str) -> &str {
    let mut parts = source.rsplitn(3, ['/', '\\']);
    let last = parts.next().unwrap_or(source);
    let Some(folder) = parts.next().filter(|_| last == "show.json") else {
        return last;
    };
    let start = source.len() - last.len() - 1 - folder.len();
    source.get(start..).unwrap_or(last)
}

#[cfg(test)]
mod tests;
