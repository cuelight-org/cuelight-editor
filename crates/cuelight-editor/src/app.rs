//! The window: an open bar with the transport, the stage beside what was
//! opened, and a status line.

use cuelight_editor_core::session::Instant;
use iced::keyboard;
use iced::widget::{
    Column, button, center, column, container, row, scrollable, shader, slider, space, text,
};
use iced::{Element, Fill, Subscription, Task, Theme};

use crate::dialog::{self, Picked};
use crate::stage::Stage;
use cuelight_editor_core::opened::{self, Opened, Summary};
use cuelight_editor_core::session::Session;

pub struct App {
    session: Option<Session>,
    /// The sound device, opened for a show that has sounds (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    audio: Option<cuelight_audio::Output>,
    /// Where the open show came from, and what it holds.
    source: String,
    summary: Summary,
    /// The last thing worth telling: an error, or what was just opened.
    status: String,
    /// A dialog is up; a second one is not opened over it.
    asking: bool,
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
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = Self {
            session: None,
            #[cfg(not(target_arch = "wasm32"))]
            audio: None,
            source: String::new(),
            summary: Summary::default(),
            status: String::new(),
            asking: false,
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
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Space),
                ..
            } => Some(Message::TogglePause),
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(c),
                modifiers,
                ..
            } => match c.as_str() {
                "r" => Some(Message::Restart),
                "," | "<" => Some(Message::Step(if modifiers.shift() {
                    -1.0
                } else {
                    -1.0 / 60.0
                })),
                "." | ">" => Some(Message::Step(if modifiers.shift() {
                    1.0
                } else {
                    1.0 / 60.0
                })),
                _ => None,
            },
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
                .push(space::horizontal().width(16))
                .push(text(&self.source).size(14));
        }

        let body: Element<'_, Message> = match &self.session {
            None => center(
                text(if cfg!(target_arch = "wasm32") {
                    "Open a packed show (.cuelight) or a show.json."
                } else {
                    "Open a show folder, a packed show (.cuelight) or a show.json, or drop one on this window."
                })
                .size(18),
            )
            .into(),
            Some(session) => {
                let stage = shader(Stage {
                    engine: session.engine.clone(),
                    revision: session.revision,
                })
                .width(Fill)
                .height(Fill);
                row![
                    container(stage).width(Fill).height(Fill),
                    scrollable(summary(&self.summary)).width(320).height(Fill),
                ]
                .into()
            }
        };

        let status = container(text(&self.status).size(13))
            .padding([4, 8])
            .width(Fill);

        column![container(bar).padding(8).width(Fill), body, status].into()
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
}
