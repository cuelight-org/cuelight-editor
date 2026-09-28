//! A show playing: the engine, its driver and the clock that moves them.
//!
//! The clock is anchored, as the players' are: every frame lands on the
//! instant that has passed since the show's time 0, so a slow frame is
//! caught up with rather than lost. Pausing keeps the show where it is
//! and moves the anchor under it when playing goes on.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cuelight::Engine;
use cuelight_core::{Event, Value};
use cuelight_loader::{Applied, Driver, DriverPlayer, Live};
use std::time::Duration;

pub use cuelight_loader::Step;
pub use web_time::Instant;

pub struct Session {
    /// Shared with the stage, which reads it on the render thread.
    pub engine: Arc<Mutex<Engine>>,
    driver: Option<Driver>,
    player: Option<DriverPlayer>,
    /// What the host fired and set live, replayed by a seek at the
    /// instants it happened.
    live: Live,
    anchor: Option<Instant>,
    /// The show's time, in seconds.
    pub time: f64,
    pub paused: bool,
    /// Bumped whenever the show moved, so a frame is redrawn only then.
    pub revision: u64,
    /// Inputs fired by hand are recorded, so a scrub replays them.
    pub recording: bool,
    /// Whether the driver plays. Off, the show waits for the hand.
    pub driving: bool,
    /// What happened lately, newest last: inputs given, driver steps
    /// applied and events the show fired, with the show time of each.
    pub happened: VecDeque<Happened>,
}

/// One thing that happened in a session.
#[derive(Debug, Clone, PartialEq)]
pub struct Happened {
    pub at: f64,
    pub what: What,
}

#[derive(Debug, Clone, PartialEq)]
pub enum What {
    /// A trigger fired by hand, by a key or by a press.
    Fired(String),
    /// A variable set by hand.
    Set(String, Value),
    /// A trigger the show fired itself: an `on_end`, a scene entered.
    Event(String),
    /// A step of the driver, applied at its own instant.
    Driver(Step),
}

/// How much of the recent past the panel shows.
const KEPT: usize = 60;

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
            recording: true,
            driving: true,
            happened: VecDeque::new(),
        }
    }

    /// Whether the show came with a driver at all.
    pub fn has_driver(&self) -> bool {
        self.driver.is_some()
    }

    /// Turn the driver on or off. Off, a seek replays only what was
    /// fired by hand; on again, the driver plays from the show's time on.
    pub fn set_driving(&mut self, on: bool, now: Instant) {
        if self.driving == on {
            return;
        }
        self.driving = on;
        let time = self.time;
        self.seek(time, now);
    }

    /// Fire a trigger as a host would, now.
    pub fn fire(&mut self, trigger: &str) {
        self.engine
            .lock()
            .expect("the engine is not poisoned")
            .trigger(trigger);
        self.fired(trigger);
    }

    /// Set a variable as a host would, now; recorded, so a scrub replays
    /// it where it was set.
    pub fn set(&mut self, name: &str, value: Value) {
        self.engine
            .lock()
            .expect("the engine is not poisoned")
            .set_variable(name, value.clone());
        if self.recording {
            self.live.record_set(self.time, name, value.clone());
        }
        self.note(What::Set(name.to_owned(), value));
    }

    /// A key, by the name a browser gives it; fires what the show says
    /// it means, if anything.
    pub fn key(&mut self, key: &str) -> Option<String> {
        let fired = self
            .engine
            .lock()
            .expect("the engine is not poisoned")
            .key(key);
        if let Some(trigger) = &fired {
            self.fired(trigger);
        }
        fired
    }

    /// A press at a canvas point; fires the pressable layer there or the
    /// show's press-anywhere trigger, if any.
    pub fn press(&mut self, at: [f64; 2]) -> Option<String> {
        let fired = self
            .engine
            .lock()
            .expect("the engine is not poisoned")
            .press(at);
        if let Some(trigger) = &fired {
            self.fired(trigger);
        }
        fired
    }

    /// What the show fired since the last look, added to `happened`.
    pub fn collect_events(&mut self) {
        let events = self
            .engine
            .lock()
            .expect("the engine is not poisoned")
            .drain_events();
        for event in events {
            #[allow(unreachable_patterns)]
            if let Event::Trigger(name) = event {
                self.note(What::Event(name));
            }
        }
    }

    /// The scene that is active now.
    pub fn active_scene(&self) -> Option<String> {
        self.engine
            .lock()
            .expect("the engine is not poisoned")
            .active_scene()
            .map(str::to_owned)
    }

    /// The current value of a variable or a show value, as the engine
    /// reads it.
    pub fn value(&self, name: &str) -> Option<Value> {
        self.engine
            .lock()
            .expect("the engine is not poisoned")
            .value(name)
    }

    fn fired(&mut self, trigger: &str) {
        if self.recording {
            self.live.record(self.time, trigger);
        }
        self.note(What::Fired(trigger.to_owned()));
        self.revision += 1;
    }

    fn note(&mut self, what: What) {
        self.note_at(self.time, what);
    }

    fn note_at(&mut self, at: f64, what: What) {
        self.happened.push_back(Happened { at, what });
        while self.happened.len() > KEPT {
            self.happened.pop_front();
        }
        self.revision += 1;
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
        let applied = match &mut self.player {
            Some(player) if self.driving => player.advance(engine.core_mut(), dt),
            _ => Vec::new(),
        };
        engine.advance_to(time);
        drop(engine);
        self.time = time;
        self.revision += 1;
        for Applied { at, step } in applied {
            if !matches!(step, Step::Wait { .. }) {
                self.note_at(at, What::Driver(step));
            }
        }
        self.collect_events();
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
    /// start, each at its own instant. Playing goes on from there;
    /// paused stays paused there.
    pub fn seek(&mut self, to: f64, now: Instant) {
        let to = to.max(0.0);
        let mut engine = self.engine.lock().expect("the engine is not poisoned");
        let driver = self.driving.then(|| self.driver.clone()).flatten();
        self.player = cuelight_loader::seek(engine.core_mut(), driver, &self.live, to);
        drop(engine);
        self.time = to;
        self.anchor = now.checked_sub(Duration::from_secs_f64(to));
        self.revision += 1;
        // A replay fires the show's own events again; they are not news.
        let _ = self
            .engine
            .lock()
            .expect("the engine is not poisoned")
            .drain_events();
    }

    /// Move by `dt` seconds, forwards or back, and stay paused there.
    pub fn step(&mut self, dt: f64, now: Instant) {
        self.paused = true;
        self.seek(self.time + dt, now);
    }

    /// How long one pass of the driver takes: the sum of its waits.
    pub fn pass_length(&self) -> Option<f64> {
        let driver = self.driver.as_ref().filter(|_| self.driving)?;
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
        self.live = Live::default();
        self.happened.clear();
        self.anchor = Some(now);
        self.time = 0.0;
        self.revision += 1;
    }
}
