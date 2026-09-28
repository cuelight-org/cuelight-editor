//! The window: an open bar with the transport, the stage beside what was
//! opened, and a status line.

use std::cell::Cell;
use std::collections::BTreeMap;

use cuelight_editor_core::assets::{Asset, Kind};
use cuelight_editor_core::inputs::{self, Inputs, Place};
use cuelight_editor_core::session::Instant;
use iced::keyboard;
use iced::widget::pane_grid::{self, Axis, Configuration};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, center, column, container, image, responsive, row, scrollable, shader, slider,
    space, svg, text, text_input, toggler,
};
use iced::{ContentFit, Element, Fill, Size, Subscription, Task, Theme};

use crate::dialog::{self, Picked};
use crate::stage::Stage;
use cuelight_editor_core::opened::{self, Opened, Summary};
use cuelight_editor_core::session::{Session, Step, What};

pub struct App {
    session: Option<Session>,
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
    /// Which asset the library shows the facts of.
    selected: Option<usize>,
    /// What the library area shows.
    tab: Tab,
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
}

/// An area of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Inputs,
    Stage,
    Library,
}

/// What the library area shows: the show's facts, or its assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Show,
    Assets,
}

/// A thumbnail of a piece of artwork: an image's own pixels, or an
/// SVG's bytes for iced to draw.
enum Thumb {
    Image(image::Handle),
    Svg(svg::Handle),
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
    /// The library area switched to a tab.
    Tab(Tab),
    /// An asset picked in the library, or the pick cleared.
    Select(Option<usize>),
    /// A split between two areas dragged.
    Resized(pane_grid::ResizeEvent),
    /// The window's scale factor, found or changed.
    Rescaled(f32),
    /// The browser decoded the show's sounds: each name with its length,
    /// or why it did not decode.
    #[cfg(target_arch = "wasm32")]
    SoundsReady(Vec<(String, Result<f64, String>)>),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = Self {
            session: None,
            audio: None,
            source: String::new(),
            summary: Summary::default(),
            library: Vec::new(),
            thumbs: Vec::new(),
            selected: None,
            tab: Tab::Show,
            inputs: Inputs::default(),
            edits: BTreeMap::new(),
            fields: BTreeMap::new(),
            status: String::new(),
            asking: false,
            panes: pane_grid::State::with_configuration(Configuration::Split {
                axis: Axis::Vertical,
                ratio: 0.24,
                a: Box::new(Configuration::Pane(Pane::Inputs)),
                b: Box::new(Configuration::Split {
                    axis: Axis::Vertical,
                    ratio: 0.7,
                    a: Box::new(Configuration::Pane(Pane::Stage)),
                    b: Box::new(Configuration::Pane(Pane::Library)),
                }),
            }),
            zoom: Zoom::Fit,
            scale_factor: 1.0,
            fitted: Cell::new(1.0),
        };
        // A path on the command line opens at once (desktop only).
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = std::env::args_os().nth(1) {
            let path = std::path::PathBuf::from(path);
            return (app, Task::done(Message::Dropped(path)));
        }
        // A page asked to open a show (`?show=<url>`) fetches it.
        #[cfg(target_arch = "wasm32")]
        {
            let mut app = app;
            app.asking = true;
            (
                app,
                Task::perform(dialog::fetch_show_from_query(), Message::Picked),
            )
        }
        #[cfg(not(target_arch = "wasm32"))]
        (app, Task::none())
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
                if let Some(session) = &mut self.session {
                    session.tick(now);
                    self.hear();
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
                self.zoom = match zoom {
                    Zoom::Fit => Zoom::Fit,
                    Zoom::Scale(scale) => self.zoom_to(scale),
                };
                Task::none()
            }
            Message::ZoomBy(factor) => {
                self.zoom = self.zoom_to(self.scale() * factor);
                Task::none()
            }
            Message::Rescaled(factor) => {
                self.scale_factor = factor;
                Task::none()
            }
            Message::Tab(tab) => {
                self.tab = tab;
                Task::none()
            }
            Message::Select(index) => {
                self.selected = index.filter(|i| *i < self.library.len());
                Task::none()
            }
            Message::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
                Task::none()
            }
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
                    ..
                } = opened;
                self.source = source;
                self.summary = summary;
                self.thumbs = thumbs(&engine, &library);
                self.library = library;
                self.selected = None;
                self.inputs = engine.show().map(Inputs::of).unwrap_or_default();
                self.edits.clear();
                let task = self.listen(&sounds, sound_files);
                self.session = Some(Session::new(engine, driver));
                // The window's scale factor bounds the zoom; ask once a
                // window is there to ask.
                let rescaled = iced::window::latest()
                    .and_then(iced::window::scale_factor)
                    .map(Message::Rescaled);
                Task::batch([task, rescaled])
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
        if sounds.is_empty() {
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

    /// Play what the show sounds like now.
    fn hear(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(audio), Some(session)) = (&self.audio, &self.session) {
            let engine = session.engine.lock().expect("the engine is not poisoned");
            match engine.voices() {
                Ok(voices) => audio.apply(&voices),
                Err(error) => log::warn!("voices: {error}"),
            }
        }
        #[cfg(target_arch = "wasm32")]
        if let (Some(audio), Some(session)) = (&self.audio, &self.session)
            && let Ok(mut audio) = audio.try_borrow_mut()
        {
            let engine = session.engine.lock().expect("the engine is not poisoned");
            match engine.voices() {
                Ok(voices) => audio.apply(&voices),
                Err(error) => log::warn!("voices: {error}"),
            }
        }
    }

    /// Silence, for a scrub or a pause.
    fn hush(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(audio) = &self.audio {
            audio.apply(&[]);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(audio) = &self.audio
            && let Ok(mut audio) = audio.try_borrow_mut()
        {
            audio.apply(&[]);
        }
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
        let mut subscriptions = vec![keys, rescaled];
        if self.session.as_ref().is_some_and(|s| !s.paused) {
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
                    Pane::Stage => responsive(move |size| self.stage(session, size)).into(),
                    Pane::Library => self.library_panel(session),
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
    /// The stage area: a zoom bar over the show drawn at its scale,
    /// centred while it fits and scrolled once it does not.
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
            on_press: Message::Press,
        })
        .width(w)
        .height(h);
        // Centred in the room it has; past that, scrolled.
        let left = ((room.width - w) / 2.0).max(0.0) + MARGIN;
        let top = ((room.height - h) / 2.0).max(0.0) + MARGIN;
        let placed = container(stage).padding([top, left]);
        let scrolled = scrollable(placed)
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
            })
            .width(Fill)
            .height(Fill);
        column![container(bar).padding([4, 8]).height(BAR), scrolled]
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// The show's inputs: triggers as buttons, variables as fields, the
    /// show's own values as readouts, and what happened lately.
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
        if !session.happened.is_empty() {
            panel = panel.push(text("HAPPENED").size(12));
            for item in session.happened.iter().rev().take(14) {
                let line = match &item.what {
                    What::Fired(t) => format!("{:6.2}  fired {t}", item.at),
                    What::Set(n, v) => format!("{:6.2}  {n} = {}", item.at, inputs::show_value(v)),
                    What::Event(t) => format!("{:6.2}  show fired {t}", item.at),
                    What::Driver(Step::Trigger { trigger }) => {
                        format!("{:6.2}  driver fired {trigger}", item.at)
                    }
                    What::Driver(Step::Set { set }) => {
                        let sets = set
                            .iter()
                            .map(|(n, v)| format!("{n} = {}", inputs::show_value(v)))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{:6.2}  driver set {sets}", item.at)
                    }
                    What::Driver(_) => continue,
                };
                panel = panel.push(text(line).size(12));
            }
        }
        panel
    }
}

impl App {
    /// The library area: a tab row over the show's facts or its assets.
    fn library_panel<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let tab = |label: &'a str, tab: Tab| {
            let mut b = button(text(label).size(13)).on_press(Message::Tab(tab));
            if self.tab == tab {
                b = b.style(button::secondary);
            }
            b
        };
        let tabs = row![tab("Show", Tab::Show), tab("Assets", Tab::Assets)].spacing(6);
        let body = match self.tab {
            Tab::Show => summary(&self.summary),
            Tab::Assets => self.assets_panel(session),
        };
        column![
            container(tabs).padding([4, 8]),
            scrollable(body).width(Fill).height(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// The assets by kind, each with its thumbnail and facts; the picked
    /// one's file and every place the show uses it right under it.
    fn assets_panel<'a>(&'a self, session: &'a Session) -> Column<'a, Message> {
        const THUMB: f32 = 40.0;
        let mut panel = Column::new().spacing(4).padding(12);
        if self.library.is_empty() {
            return panel.push(text("This show ships no assets.").size(14));
        }
        let engine = session.engine.lock().expect("the engine is not poisoned");
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
            let mut facts = match asset.kind {
                Kind::Image => asset
                    .size
                    .map(|[w, h]| format!("{w} x {h} px"))
                    .unwrap_or_default(),
                Kind::Vector => asset
                    .size
                    .map(|[w, h]| format!("{w} x {h}, vector"))
                    .unwrap_or_else(|| "vector".to_owned()),
                Kind::Font => "font".to_owned(),
                Kind::Sound => engine
                    .sound_duration(&asset.name)
                    .map(|d| format!("{d:.2} s"))
                    .unwrap_or_else(|| "sound".to_owned()),
                Kind::Video => engine
                    .video(&asset.name)
                    .map(|v| format!("{:.2} s, {} x {}", v.duration, v.width, v.height))
                    .unwrap_or_else(|| "video".to_owned()),
            };
            if asset.uses.is_empty() {
                facts.push_str(", unused");
            }
            let line = row![
                thumb,
                column![text(&asset.name).size(14), text(facts).size(12)].spacing(2)
            ]
            .spacing(8)
            .align_y(iced::Center);
            let mut b = button(line)
                .on_press(Message::Select(Some(i)))
                .width(Fill)
                .style(button::text);
            if self.selected == Some(i) {
                b = b.style(button::secondary);
            }
            panel = panel.push(b);
            if self.selected == Some(i) {
                panel = panel.push(container(picked(asset)).padding([4, 12]));
            }
        }
        panel
    }
}

/// The picked asset's file and every place the show uses it.
fn picked(asset: &Asset) -> Column<'_, Message> {
    let mut facts = Column::new().spacing(3);
    if let Some(file) = &asset.file {
        facts = facts.push(text(file).size(12));
    }
    facts = facts.push(text("USED BY").size(12));
    if asset.uses.is_empty() {
        facts = facts.push(text("nothing in this show").size(13));
    }
    for used in &asset.uses {
        facts = facts.push(row![text(&used.place).size(13), text(&used.how).size(12)].spacing(8));
    }
    facts
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

#[allow(dead_code)]
pub fn theme(_: &App) -> Theme {
    Theme::Dark
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuelight_core::Value;
    use iced_test::simulator;

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
        assert!(ui.find("Play").is_ok(), "an opened show stands at 0");
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
            assert!(ui.find("8 x 8 px").is_ok(), "an image's size");
            assert!(ui.find("USED BY").is_err(), "nothing picked yet");
        }
        assert!(matches!(app.thumbs.as_slice(), [Some(Thumb::Image(_))]));
        let _ = app.update(Message::Select(Some(0)));
        let mut ui = simulator(app.view());
        assert!(ui.find("assets/dot.png").is_ok(), "the file");
        assert!(ui.find("group/dot").is_ok(), "the layer that uses it");
        assert!(ui.find("image layer").is_ok());
        let _ = ui.click("Show");
        for message in ui.into_messages() {
            let _ = app.update(message);
        }
        assert_eq!(app.tab, Tab::Show);
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
        assert!(session.paused, "the show still stands where it opened");
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
