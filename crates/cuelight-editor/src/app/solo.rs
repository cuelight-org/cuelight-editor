//! Solo: the picked layer alone on the stage, on its own clock, with a
//! bar of its own to play it, fire what it listens to, render it to a
//! strip and save it as a show.
//!
//! A solo is a view: the tree and the inspector still edit the show,
//! and the solo loads the subtree again after each edit. The stage
//! shows the solo instead of the show, and a click on it is a press on
//! the solo; leaving the solo gives the show back, paused where it was.
//! The solo is silent.

use cuelight_editor_core::session::{Instant, lock};
use cuelight_editor_core::solo::Solo;
use iced::widget::Widget as _;
#[cfg(not(target_arch = "wasm32"))]
use iced::widget::text_input;
use iced::widget::{button, row, text};
use iced::{Element, Task};

use super::{App, Message};

/// The most frames a strip takes, and how many go in a row of it.
#[cfg(not(target_arch = "wasm32"))]
const MOST_FRAMES: usize = 64;
#[cfg(not(target_arch = "wasm32"))]
const ROW: usize = 8;
/// The room round the subtree in each frame of a strip, in canvas pixels.
#[cfg(not(target_arch = "wasm32"))]
const MARGIN: f64 = 8.0;

impl App {
    /// Solo the layer picked last, playing from 0.
    pub(super) fn solo(&mut self) -> Task<Message> {
        let (Some(session), Some(document), Some(path)) =
            (&mut self.session, &self.document, self.selection.last())
        else {
            return Task::none();
        };
        session.paused = true;
        let engine = lock(&session.engine);
        let Some(show) = engine.show() else {
            return Task::none();
        };
        let sounds: Vec<(String, f64)> = self
            .library
            .iter()
            .filter_map(|asset| Some((asset.name.clone(), engine.sound_duration(&asset.name)?)))
            .collect();
        let opened = Solo::open(&self.files, &document.value(), show, path, &sounds);
        drop(engine);
        match opened {
            Ok(mut solo) => {
                solo.session.toggle_pause(Instant::now());
                self.status = format!("soloed {}", solo.place);
                self.solo = Some(solo);
            }
            Err(error) => self.status = format!("could not solo: {error}"),
        }
        self.hush();
        Task::none()
    }

    /// The show was edited: the solo loads its subtree again, or ends
    /// when it is gone.
    pub(super) fn reload_solo(&mut self) {
        let (Some(solo), Some(document)) = (&mut self.solo, &self.document) else {
            return;
        };
        if let Err(error) = solo.reload(&document.value(), Instant::now()) {
            self.status = format!("left the solo: {error}");
            self.solo = None;
        }
    }

    /// What the solo's own messages do.
    pub(super) fn solo_message(&mut self, message: SoloMessage) -> Task<Message> {
        if let SoloMessage::Enter = message {
            return self.solo();
        }
        let Some(solo) = &mut self.solo else {
            return Task::none();
        };
        let now = Instant::now();
        match message {
            SoloMessage::Enter => {}
            SoloMessage::Leave => {
                self.solo = None;
                self.status = "back to the show".to_owned();
            }
            SoloMessage::TogglePause => solo.session.toggle_pause(now),
            SoloMessage::Restart => solo.session.restart(now),
            SoloMessage::Fire(trigger) => solo.session.fire(&trigger),
            SoloMessage::Press(at) => {
                solo.session.press(at);
            }
            SoloMessage::Frames(typed) => self.strip_frames = typed,
            SoloMessage::Seconds(typed) => self.strip_seconds = typed,
            #[cfg(not(target_arch = "wasm32"))]
            SoloMessage::Render => {
                let name = format!("{}-strip.png", solo.place.replace(['/', ' ', ':'], "_"));
                return Task::perform(
                    crate::dialog::save_to(
                        "Render the solo to a strip",
                        name,
                        self.show_folder(),
                        ("PNG image", &["png"]),
                    ),
                    |to| Message::Solo(SoloMessage::RenderTo(to)),
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            SoloMessage::RenderTo(Some(to)) => {
                self.status = match self.render_strip(&to) {
                    Ok(frames) => format!("rendered {frames} frame(s) to {}", to.display()),
                    Err(error) => format!("could not render the strip: {error}"),
                };
            }
            SoloMessage::Save => {
                let name = format!(
                    "{}.{}",
                    solo.place.replace(['/', ' ', ':'], "_"),
                    cuelight_editor_core::solo::PACK_EXTENSION
                );
                #[cfg(not(target_arch = "wasm32"))]
                return Task::perform(
                    crate::dialog::save_to(
                        "Save the solo as a show",
                        name,
                        self.show_folder(),
                        ("cuelight show", &["cuelight"]),
                    ),
                    |to| Message::Solo(SoloMessage::SaveTo(to)),
                );
                #[cfg(target_arch = "wasm32")]
                {
                    self.status = match self
                        .solo_pack()
                        .and_then(|bytes| crate::dialog::offer_download(&name, &bytes))
                    {
                        Ok(()) => format!("downloaded {name}"),
                        Err(error) => format!("could not save the solo: {error}"),
                    };
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            SoloMessage::SaveTo(Some(to)) => {
                self.status = match self
                    .solo_pack()
                    .and_then(|bytes| std::fs::write(&to, bytes).map_err(|e| e.to_string()))
                {
                    Ok(()) => format!("saved the solo as {}", to.display()),
                    Err(error) => format!("could not save the solo: {error}"),
                };
            }
            #[cfg(not(target_arch = "wasm32"))]
            SoloMessage::RenderTo(None) | SoloMessage::SaveTo(None) => {}
        }
        Task::none()
    }

    /// The solo as a packed show: its document and the assets it uses.
    fn solo_pack(&self) -> Result<Vec<u8>, String> {
        let solo = self.solo.as_ref().ok_or("nothing is soloed")?;
        solo.pack(&self.files, &self.library)
    }

    /// The folder the open show is in, or is, where what is made of it
    /// is offered to go.
    #[cfg(not(target_arch = "wasm32"))]
    fn show_folder(&self) -> Option<std::path::PathBuf> {
        use cuelight_editor_core::save::Origin;
        match self.origin.as_ref()? {
            Origin::Folder(dir) => Some(dir.clone()),
            Origin::Pack(path) | Origin::Loose(path) => path.parent().map(ToOwned::to_owned),
            Origin::Bytes { .. } => None,
        }
    }

    /// Render the solo to `to`: the frames the strip fields ask for, from
    /// the solo's playhead on, each cut to the box round the subtree in
    /// all of them, side by side, a row of [`ROW`] at a time. What was
    /// fired by hand is replayed, so a strip shows it. The solo is put
    /// back where it was. How many frames were written.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn render_strip(&mut self, to: &std::path::Path) -> Result<usize, String> {
        use cuelight::render::{Renderer, RgbaFrame};
        use cuelight_editor_core::solo::{crop, sheet, strip_times};

        let count = self
            .strip_frames
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=MOST_FRAMES).contains(n))
            .ok_or(format!("frames is a count from 1 to {MOST_FRAMES}"))?;
        let seconds = self
            .strip_seconds
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|s| s.is_finite() && *s >= 0.0)
            .ok_or("seconds is a length of time, 0 or more")?;
        let solo = self.solo.as_mut().ok_or("nothing is soloed")?;
        let mut renderer = Renderer::new().map_err(|e| e.to_string())?;
        let (from, paused) = (solo.session.time, solo.session.paused);
        solo.session.paused = true;
        let mut frames = Vec::new();
        let mut boxes = Vec::new();
        let mut size = [0, 0];
        let mut failed = None;
        for at in strip_times(from, seconds, count) {
            solo.session.seek(at, Instant::now());
            let rendered = renderer.render_to_rgba(&lock(&solo.session.engine));
            match rendered {
                Ok(frame) => {
                    size = [frame.width, frame.height];
                    frames.push((frame.width, frame.pixels));
                }
                Err(error) => {
                    failed = Some(error.to_string());
                    break;
                }
            }
            boxes.extend(solo.area());
        }
        solo.session.seek(from, Instant::now());
        solo.session.paused = paused;
        if let Some(error) = failed {
            return Err(error);
        }
        let area = crop(&boxes, MARGIN, size).ok_or("the solo draws nothing")?;
        let (width, height, pixels) = sheet(&frames, area, count.min(ROW));
        RgbaFrame {
            width,
            height,
            pixels,
        }
        .write_png(to)
        .map_err(|e| e.to_string())?;
        Ok(frames.len())
    }

    /// The bar over the stage while soloing: what is soloed, its clock,
    /// a button for each trigger it listens to, the strip, and leaving.
    pub(super) fn solo_bar<'a>(&'a self, solo: &'a Solo) -> Element<'a, Message> {
        let say = |m: SoloMessage| Message::Solo(m);
        let small = |label: &'a str| text(label).size(13);
        let mut bar = row![
            text(format!("SOLO {}", solo.place)).size(13),
            button(small("|<")).on_press(say(SoloMessage::Restart)),
            button(small(if solo.session.paused { "Play" } else { "Pause" }))
                .on_press(say(SoloMessage::TogglePause)),
            text(format!("{:6.2} s", solo.session.time)).size(13),
        ]
        .spacing(6)
        .align_y(iced::Center);
        for trigger in &solo.triggers {
            bar = bar.push(
                button(text(trigger).size(13))
                    .on_press(say(SoloMessage::Fire(trigger.clone())))
                    .style(button::secondary)
                    .boxed(),
            );
        }
        bar = bar.push(iced::widget::space::horizontal().boxed());
        #[cfg(not(target_arch = "wasm32"))]
        {
            bar = bar
                .push(
                    text_input("frames", &self.strip_frames)
                        .on_input(|t| Message::Solo(SoloMessage::Frames(t)))
                        .size(13)
                        .width(56)
                        .boxed(),
                )
                .push(small("frames over").boxed())
                .push(
                    text_input("seconds", &self.strip_seconds)
                        .on_input(|t| Message::Solo(SoloMessage::Seconds(t)))
                        .size(13)
                        .width(56)
                        .boxed(),
                )
                .push(small("s").boxed())
                .push(
                    button(small("Render strip..."))
                        .on_press(say(SoloMessage::Render))
                        .boxed(),
                );
        }
        bar.push(
            button(small("Save as show..."))
                .on_press(say(SoloMessage::Save))
                .boxed(),
        )
        .push(
            button(small("Leave solo"))
                .on_press(say(SoloMessage::Leave))
                .boxed(),
        )
        .wrap()
        .vertical_spacing(4)
        .boxed()
    }
}

/// What is done to the solo.
#[derive(Debug, Clone)]
pub enum SoloMessage {
    /// Solo the layer picked last.
    Enter,
    /// Back to the show, paused where it was.
    Leave,
    TogglePause,
    Restart,
    /// A trigger fired from the solo's bar.
    Fire(String),
    /// A click on the stage while soloing: a press on the solo.
    Press([f64; 2]),
    /// The strip's frame count and length, as typed.
    Frames(String),
    Seconds(String),
    /// Ask where the strip goes, then render it there.
    #[cfg(not(target_arch = "wasm32"))]
    Render,
    #[cfg(not(target_arch = "wasm32"))]
    RenderTo(Option<std::path::PathBuf>),
    /// Ask where the solo's show goes, then write it there; a browser
    /// downloads it.
    Save,
    #[cfg(not(target_arch = "wasm32"))]
    SaveTo(Option<std::path::PathBuf>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuelight_core::{LayerPath, Property, Root};
    use cuelight_editor_core::session::What;
    use cuelight_editor_core::tree::{self, Row};
    use iced_test::simulator;

    fn mini() -> App {
        let (mut app, _) = App::new();
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cuelight-editor-core/tests/fixtures/mini"
        );
        let _ = app.update(Message::Dropped(dir.into()));
        app
    }

    #[test]
    fn a_soloed_layer_plays_alone_and_follows_the_edits() {
        let mut app = mini();
        let dot = LayerPath::new(Root::Show, [1, 0]);
        let _ = app.update(Message::Choose(dot.clone()));
        let _ = app.update(Message::Solo(SoloMessage::Enter));
        let solo = app.solo.as_ref().expect("soloed");
        assert_eq!(solo.place, "group/dot");
        assert!(!solo.session.paused, "a solo plays");
        assert!(app.session.as_ref().unwrap().paused, "the show waits");

        // The solo's bar fires its trigger on the solo, not the show.
        assert!(simulator(app.view()).find("SOLO group/dot").is_ok());
        let messages = {
            let mut ui = simulator(app.solo_bar(app.solo.as_ref().unwrap()));
            let _ = ui.click("go");
            ui.into_messages().collect::<Vec<_>>()
        };
        for message in messages {
            let _ = app.update(message);
        }
        let solo = app.solo.as_ref().unwrap();
        assert!(
            solo.session
                .happened
                .iter()
                .any(|h| h.what == What::Fired("go".to_owned()))
        );
        assert!(app.session.as_ref().unwrap().happened.is_empty());

        // An edit goes to the show, and the solo shows it.
        let _ = app.update(Message::Type(Property::X, "3".to_owned()));
        let _ = app.update(Message::Apply(Property::X));
        assert_eq!(
            app.document.as_ref().unwrap().value()["layers"][1]["children"][0]["x"],
            3
        );
        let solo = app.solo.as_ref().unwrap();
        assert_eq!(solo.document["layers"][0]["children"][0]["x"], 3);

        let _ = app.update(Message::Solo(SoloMessage::Leave));
        assert!(app.solo.is_none());
    }

    /// The picture book's wolf: soloed, its `tap_wolf` fired from the
    /// solo's own button, and rendered to a strip of the wolf alone.
    #[test]
    fn the_wolf_is_soloed_fired_and_rendered_to_a_strip() {
        let Some(book) = super::super::tests::examples().map(|e| e.join("demos/red_riding_hood"))
        else {
            eprintln!("no cuelight-examples checkout: the wolf is not soloed");
            return;
        };
        let (mut app, _) = App::new();
        let _ = app.update(Message::Dropped(book));
        let wolf = {
            let session = app.session.as_ref().unwrap();
            let engine = lock(&session.engine);
            let show = engine.show().unwrap();
            app.rows.iter().find_map(|row| match row {
                Row::Layer { path, name, .. }
                    if name == "wolf"
                        && tree::layer(show, path).is_some_and(|l| {
                            l.timelines
                                .iter()
                                .any(|t| t.trigger.iter().any(|t| t == "tap_wolf"))
                        }) =>
                {
                    Some(path.clone())
                }
                _ => None,
            })
        }
        .expect("a wolf that hears tap_wolf");
        let _ = app.update(Message::Choose(wolf));
        let _ = app.update(Message::Solo(SoloMessage::Enter));
        let _ = app.update(Message::Solo(SoloMessage::TogglePause));
        let messages = {
            let mut ui = simulator(app.solo_bar(app.solo.as_ref().expect("soloed")));
            let _ = ui.click("tap_wolf");
            ui.into_messages().collect::<Vec<_>>()
        };
        for message in messages {
            let _ = app.update(message);
        }
        let solo = app.solo.as_ref().expect("soloed");
        assert!(
            solo.session
                .happened
                .iter()
                .any(|h| h.what == What::Fired("tap_wolf".to_owned()))
        );

        app.strip_frames = "4".to_owned();
        app.strip_seconds = "0.6".to_owned();
        let dir = tempfile::tempdir().unwrap();
        let strip = dir.path().join("wolf.png");
        match app.render_strip(&strip) {
            Ok(frames) => assert_eq!(frames, 4),
            Err(error) if error.contains("adapter") || error.contains("evice") => {
                eprintln!("no GPU to render with: {error}");
                return;
            }
            Err(error) => panic!("{error}"),
        }
        let decoder = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(&strip).unwrap(),
        ));
        let info = decoder.read_info().unwrap();
        let (w, h) = (info.info().width, info.info().height);
        // Four wolves in a row, each far smaller than the page.
        assert!(w % 4 == 0 && w / 4 < 640 && h < 520, "{w} x {h}");
        let solo = app.solo.as_ref().unwrap();
        assert!(solo.session.paused, "the solo is put back as it was");
    }
}
