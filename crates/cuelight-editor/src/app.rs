//! The window: an open bar with the transport, the stage beside what was
//! opened, and a status line.

use std::cell::Cell;
use std::collections::BTreeMap;

use cuelight_editor_core::inputs::{self, Inputs, Place};
use cuelight_editor_core::session::Instant;
use iced::keyboard;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, center, column, container, responsive, row, scrollable, shader, slider, space,
    text, text_input, toggler,
};
use iced::{Element, Fill, Size, Subscription, Task, Theme};

use crate::dialog::{self, Picked};
use crate::stage::Stage;
use cuelight_editor_core::opened::{self, Opened, Summary};
use cuelight_editor_core::session::{Session, What};

pub struct App {
    session: Option<Session>,
    /// The sound device, opened for a show that has sounds (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    audio: Option<cuelight_audio::Output>,
    /// Where the open show came from, and what it holds.
    source: String,
    summary: Summary,
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
    /// How large the stage draws the show.
    zoom: Zoom,
    /// The scale that fits the show into the stage area, as the last
    /// layout found it: what zooming in or out starts from while fitted.
    fitted: Cell<f32>,
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

    fn scaled(scale: f32) -> Self {
        Self::Scale(scale.clamp(Self::MIN, Self::MAX))
    }
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
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = Self {
            session: None,
            #[cfg(not(target_arch = "wasm32"))]
            audio: None,
            source: String::new(),
            summary: Summary::default(),
            inputs: Inputs::default(),
            edits: BTreeMap::new(),
            fields: BTreeMap::new(),
            status: String::new(),
            asking: false,
            zoom: Zoom::Fit,
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
                    None => {}
                }
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::Dropped(path) => {
                self.open(Opened::from_path(&path));
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
                self.zoom = zoom;
                Task::none()
            }
            Message::ZoomBy(factor) => {
                self.zoom = Zoom::scaled(self.scale() * factor);
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

    fn open(&mut self, result: Result<Opened, opened::OpenError>) {
        match result {
            Ok(opened) => {
                self.status = format!("opened {}", opened.source);
                log::info!("{}", self.status);
                let Opened {
                    source,
                    engine,
                    summary,
                    driver,
                    sounds,
                } = opened;
                self.source = source;
                self.summary = summary;
                self.inputs = engine.show().map(Inputs::of).unwrap_or_default();
                self.edits.clear();
                self.listen(&sounds);
                self.session = Some(Session::new(engine, driver));
            }
            Err(error) => {
                self.status = format!("could not open: {error}");
                log::warn!("{}", self.status);
            }
        }
    }

    /// Give the sound device the show's sounds, opening it for the first
    /// show that has any.
    #[cfg(not(target_arch = "wasm32"))]
    fn listen(&mut self, sounds: &[(String, std::sync::Arc<cuelight_editor_core::opened::Sound>)]) {
        if sounds.is_empty() {
            return;
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
    }

    #[cfg(target_arch = "wasm32")]
    fn listen(
        &mut self,
        _sounds: &[(String, std::sync::Arc<cuelight_editor_core::opened::Sound>)],
    ) {
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
    }

    /// Silence, for a scrub or a pause.
    fn hush(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(audio) = &self.audio {
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
        let mut subscriptions = vec![keys];
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
                    "Open a packed show (.cuelight)."
                } else {
                    "Open a show folder, a packed show (.cuelight) or a show.json, or drop one on this window."
                })
                .size(18),
            )
            .into(),
            Some(session) => row![
                scrollable(self.inputs_panel(session)).width(260).height(Fill),
                responsive(move |size| self.stage(session, size)),
                scrollable(summary(&self.summary)).width(300).height(Fill),
            ]
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
                    cuelight::Value::Bool(on) => toggler(on)
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
                };
                panel = panel.push(text(line).size(12));
            }
        }
        panel
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
        assert!(ui.find("Pause").is_ok(), "an opened show plays");
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
    fn a_tick_moves_the_show_and_pausing_holds_it() {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        let start = Instant::now();
        let _ = app.update(Message::Tick(start));
        let _ = app.update(Message::Tick(start + std::time::Duration::from_millis(500)));
        let time = app.session.as_ref().unwrap().time;
        assert!((time - 0.5).abs() < 1e-9, "{time}");
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
        assert!(!session.paused);
        assert!(
            matches!(session.happened.back().map(|h| &h.what), Some(What::Fired(t)) if t == "go")
        );

        // With Ctrl, the editor's own Space pauses.
        let _ = app.update(Message::KeyPressed(
            keyboard::Key::Named(keyboard::key::Named::Space),
            keyboard::Modifiers::CTRL,
        ));
        assert!(app.session.as_ref().unwrap().paused);

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
            Some(cuelight::Value::Bool(true))
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
            Some(cuelight::Value::Bool(false))
        );
        let _ = app.update(Message::Drive(true));
        let _ = app.update(Message::Seek(1.0));
        assert_eq!(
            app.session.as_ref().unwrap().value("lit"),
            Some(cuelight::Value::Bool(true))
        );
    }
}
