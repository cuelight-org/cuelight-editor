//! A show playing: the engine, its driver and the clock that moves them.
//!
//! The clock is anchored, as the players' are: every frame lands on the
//! instant that has passed since the show's time 0, so a slow frame is
//! caught up with rather than lost. Pausing keeps the show where it is
//! and moves the anchor under it when playing goes on.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cuelight::{Engine, Pressed};
use cuelight_core::{Event, Finding, Value};
use cuelight_loader::{Applied, Driver, DriverPlayer, Live};
use std::time::Duration;

use crate::log::{self, Log};

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
    /// The log below the stage: what is said of the document, and what
    /// happened while the show played, one line each, newest last.
    pub log: Log,
    /// The paths of the show's files, for the audit to know what is
    /// there; empty when the show came without a folder.
    pub files: Vec<String>,
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
    /// A press asked for a web address to be opened; the editor only
    /// notes it.
    Opened(String),
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
            // A show opens paused at 0; playing is asked for.
            paused: true,
            revision: 0,
            recording: true,
            driving: true,
            happened: VecDeque::new(),
            log: Log::default(),
            files: Vec::new(),
        }
    }

    /// Audit `json`, the document as written, against the show's files
    /// and its driver, and put what it says in the log in place of the
    /// last audit.
    pub fn audit(&mut self, json: &str) {
        self.log.clear_audit();
        self.log
            .extend(log::audit(json, &self.files, self.driver.as_ref()));
        self.revision += 1;
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
        self.settle();
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
    /// show's press-anywhere trigger, if any. A layer that opens a web
    /// address has that noted and nothing opened: the editor is not the
    /// kiosk.
    pub fn press(&mut self, at: [f64; 2]) -> Option<Pressed> {
        let pressed = self
            .engine
            .lock()
            .expect("the engine is not poisoned")
            .press(at)?;
        if let Some(trigger) = &pressed.trigger {
            self.fired(trigger);
        }
        if let Some(url) = &pressed.open {
            self.note(What::Opened(url.clone()));
        }
        Some(pressed)
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
        self.settle();
    }

    /// An input given while paused is applied at the paused instant and
    /// the show stays there: the engine runs a step that does not move
    /// its clock, as a seek landing on that instant would, so the scene
    /// entered and the timelines started are on the stage at once.
    fn settle(&mut self) {
        if self.paused {
            self.engine
                .lock()
                .expect("the engine is not poisoned")
                .advance_to(self.time);
            self.collect_events();
            self.collect_trace();
        }
        self.revision += 1;
    }

    fn note(&mut self, what: What) {
        self.note_at(self.time, what);
    }

    fn note_at(&mut self, at: f64, what: What) {
        let line = match &what {
            What::Fired(trigger) => Some(log::fired(at, trigger)),
            What::Set(name, value) => Some(log::set(at, name, value)),
            What::Opened(url) => Some(log::opened(at, url)),
            What::Driver(step) => log::driver(at, step),
            // The trace says this in the engine's own words.
            What::Event(_) => None,
        };
        if let Some(line) = line {
            self.log.push(line);
        }
        self.happened.push_back(Happened { at, what });
        while self.happened.len() > KEPT {
            self.happened.pop_front();
        }
        self.revision += 1;
    }

    /// What the engine traced since the last look, added to the log in
    /// the trace's own words.
    pub fn collect_trace(&mut self) {
        let traced = self
            .engine
            .lock()
            .expect("the engine is not poisoned")
            .drain_trace();
        if traced.is_empty() {
            return;
        }
        self.log.extend(traced.iter().map(log::traced));
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
        self.collect_trace();
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
        // A replay fires the show's own events again and traces every
        // step of it; neither is news.
        let mut engine = self.engine.lock().expect("the engine is not poisoned");
        let _ = engine.drain_events();
        let _ = engine.drain_trace();
    }

    /// The show changed under the session: load the new document, as
    /// much of it as loads, and put it back at the playhead by replaying
    /// its inputs. Assets stay registered, so this costs the parse and
    /// the replay. What the load dropped comes back as findings; only a
    /// text that is no document at all is an error, and then the show
    /// that was playing stays.
    ///
    /// The document is not audited here: an audit of a large show takes
    /// longer than a frame, so the caller runs [`Session::audit`] once the
    /// show is back on screen.
    pub fn reload(
        &mut self,
        text: &str,
        now: Instant,
    ) -> Result<Vec<Finding>, cuelight_core::Error> {
        let findings = self
            .engine
            .lock()
            .expect("the engine is not poisoned")
            .load_show_tolerant(text)?;
        let time = self.time;
        let paused = self.paused;
        self.seek(time, now);
        self.paused = paused;
        Ok(findings)
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
        self.log.clear_played();
        let _ = self
            .engine
            .lock()
            .expect("the engine is not poisoned")
            .drain_trace();
        self.anchor = Some(now);
        self.time = 0.0;
        self.revision += 1;
    }
}
