//! Sound: the show's voices and a previewed one, handed to the sound
//! device or the browser's audio.

use iced::Task;

#[cfg(not(target_arch = "wasm32"))]
use super::OPTIONS;
use super::{App, Message};

/// The voice a preview plays under. The engine's own voice ids count up
/// from one and never reach this.
const PREVIEW_VOICE: u64 = u64::MAX;

impl App {
    /// Give the sound device the show's sounds, opening it for the first
    /// show that has any.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn listen(
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
    pub(super) fn listen(
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
    pub(super) fn hear(&mut self) {
        let voices = self.voices(true);
        self.play(&voices);
    }

    /// Silence the show, for a scrub or a pause; a preview plays on.
    pub(super) fn hush(&mut self) {
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
    pub(super) fn can_play(&self) -> bool {
        self.audio.is_some()
    }
}
