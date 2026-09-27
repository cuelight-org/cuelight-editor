//! The window: an open bar, what was opened, and a status line.

use iced::widget::{Column, button, center, column, container, row, scrollable, space, text};
use iced::{Element, Fill, Subscription, Task, Theme};

use crate::dialog::{self, Picked};
use crate::opened::{self, Opened};

pub struct App {
    opened: Option<Opened>,
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
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = Self {
            opened: None,
            status: String::new(),
            asking: false,
        };
        // A path on the command line opens at once (desktop only).
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = std::env::args_os().nth(1) {
            let path = std::path::PathBuf::from(path);
            return (app, Task::done(Message::Dropped(path)));
        }
        (app, Task::none())
    }

    pub fn title(&self) -> String {
        match &self.opened {
            Some(opened) => format!("{} - cuelight editor", opened.summary.name),
            None => "cuelight editor".to_owned(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenFile => self.ask(dialog::pick_file()),
            #[cfg(not(target_arch = "wasm32"))]
            Message::OpenFolder => self.ask(dialog::pick_folder()),
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
        }
    }

    fn ask(
        &mut self,
        dialog: impl Future<Output = Option<Picked>> + Send + 'static,
    ) -> Task<Message> {
        if self.asking {
            return Task::none();
        }
        self.asking = true;
        Task::perform(dialog, Message::Picked)
    }

    fn open(&mut self, result: Result<Opened, opened::OpenError>) {
        match result {
            Ok(opened) => {
                self.status = format!("opened {}", opened.source);
                log::info!("{}", self.status);
                self.opened = Some(opened);
            }
            Err(error) => {
                self.status = format!("could not open: {error}");
                log::warn!("{}", self.status);
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                    Some(Message::Dropped(path))
                }
                _ => None,
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Subscription::none()
        }
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
        if let Some(opened) = &self.opened {
            bar = bar
                .push(space::horizontal())
                .push(text(&opened.source).size(14));
        }

        let body: Element<'_, Message> = match &self.opened {
            None => center(
                text(if cfg!(target_arch = "wasm32") {
                    "Open a packed show (.cuelight) or a show.json."
                } else {
                    "Open a show folder, a packed show (.cuelight) or a show.json, or drop one on this window."
                })
                .size(18),
            )
            .into(),
            Some(opened) => scrollable(summary(opened)).height(Fill).into(),
        };

        let status = container(text(&self.status).size(13))
            .padding([4, 8])
            .width(Fill);

        column![container(bar).padding(8).width(Fill), body, status].into()
    }
}

fn summary(opened: &Opened) -> Column<'_, Message> {
    let mut rows = Column::new().spacing(6).padding(16);
    for (label, value) in opened::lines(&opened.summary) {
        rows = rows.push(row![text(label).size(14).width(110), text(value).size(14)].spacing(8));
    }
    if !opened.summary.problems.is_empty() {
        rows = rows.push(text(format!("{} problem(s)", opened.summary.problems.len())).size(14));
        for problem in &opened.summary.problems {
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
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mini");
        let _ = app.update(Message::Dropped(dir.into()));
        assert!(app.status.starts_with("opened "), "{}", app.status);
        let mut ui = simulator(app.view());
        assert!(ui.find("mini (format 1)").is_ok());
        assert!(ui.find("64 x 32").is_ok());
    }
}
