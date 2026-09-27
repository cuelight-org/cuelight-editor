//! A show playing: the engine, its driver and the clock that moves them.
//!
//! The clock is anchored, as the players' are: every frame lands on the
//! instant that has passed since the show's time 0, so a slow frame is
//! caught up with rather than lost. Pausing keeps the show where it is
//! and moves the anchor under it when playing goes on.

use std::sync::{Arc, Mutex};

use cuelight::Engine;
use cuelight_loader::{Driver, DriverPlayer, Live, Step};
use std::time::Duration;

pub use web_time::Instant;

pub struct Session {
    /// Shared with the stage, which reads it on the render thread.
    pub engine: Arc<Mutex<Engine>>,
    driver: Option<Driver>,
    player: Option<DriverPlayer>,
    /// What the host fired live, replayed by a seek (nothing yet: keys
    /// and presses come with the inputs panel).
    live: Live,
    anchor: Option<Instant>,
    /// The show's time, in seconds.
    pub time: f64,
    pub paused: bool,
    /// Bumped whenever the show moved, so a frame is redrawn only then.
    pub revision: u64,
}

impl Session {
    pub fn new(engine: Engine, driver: Option<Driver>) -> Self {
        Self {
            engine: Arc::new(Mutex::new(engine)),
            player: driver.clone().map(DriverPlayer::new),
            driver,
            live: Live::default(),
            anchor: None,
            time: 0.0,
            paused: false,
            revision: 0,
        }
    }

    /// A frame: move the show to the instant `now` stands for.
    pub fn tick(&mut self, now: Instant) {
        if self.paused {
            return;
        }
        let anchor = *self.anchor.get_or_insert(now);
        let time = now.saturating_duration_since(anchor).as_secs_f64();
        let dt = time - self.time;
        if dt <= 0.0 {
            return;
        }
        let mut engine = self.engine.lock().expect("the engine is not poisoned");
        if let Some(player) = &mut self.player {
            player.advance(&mut engine, dt);
        }
        engine.advance_to(time);
        self.time = time;
        self.revision += 1;
    }

    pub fn toggle_pause(&mut self, now: Instant) {
        if self.paused {
            // Re-anchor under the show's current time, so it goes on
            // from where it stopped.
            self.anchor = now.checked_sub(Duration::from_secs_f64(self.time));
            self.paused = false;
        } else {
            self.paused = true;
        }
    }

    /// Put the show at `to` seconds, by replaying its inputs from the
    /// start. Playing goes on from there; paused stays paused there.
    pub fn seek(&mut self, to: f64, now: Instant) {
        let to = to.max(0.0);
        let mut engine = self.engine.lock().expect("the engine is not poisoned");
        self.player = cuelight_loader::seek(&mut engine, self.driver.clone(), &self.live, to, 60.0);
        drop(engine);
        self.time = to;
        self.anchor = now.checked_sub(Duration::from_secs_f64(to));
        self.revision += 1;
    }

    /// Move by `dt` seconds, forwards or back, and stay paused there.
    pub fn step(&mut self, dt: f64, now: Instant) {
        self.paused = true;
        self.seek(self.time + dt, now);
    }

    /// How long one pass of the driver takes: the sum of its waits.
    pub fn pass_length(&self) -> Option<f64> {
        let driver = self.driver.as_ref()?;
        let waits = driver
            .steps
            .iter()
            .map(|step| match step {
                Step::Wait { wait } => *wait,
                _ => 0.0,
            })
            .sum::<f64>();
        (waits > 0.0).then_some(waits)
    }

    pub fn restart(&mut self, now: Instant) {
        self.engine
            .lock()
            .expect("the engine is not poisoned")
            .restart();
        self.player = self.driver.clone().map(DriverPlayer::new);
        self.anchor = Some(now);
        self.time = 0.0;
        self.revision += 1;
    }
}
