//! How long a seek takes: replaying a show's driver at 60 fps against
//! jumping from one input to the next, and whether both land on the
//! same frame. Also what a reload costs. The numbers decide whether
//! the editor needs engine snapshots (spec, M0 item 6).
//!
//!     cargo run --release --example seek_timing -- ../cuelight-examples/deck [more shows]

use std::collections::BTreeMap;
use std::time::Instant;

use cuelight::Engine;
use cuelight_loader::{Driver, Live, Step, load, seek};

/// The driver's inputs at absolute times, looping if it loops, up to
/// `until` seconds.
fn inputs(driver: &Driver, until: f64) -> Vec<(f64, Step)> {
    let mut out = Vec::new();
    let mut time = 0.0;
    loop {
        for step in &driver.steps {
            match step {
                Step::Wait { wait } => time += wait,
                other => out.push((time, other.clone())),
            }
            if time > until {
                return out;
            }
        }
        if !driver.looping || driver.steps.is_empty() {
            return out;
        }
    }
}

/// Seek by jumping: the show advances straight to each input's instant.
fn jump(engine: &mut Engine, inputs: &[(f64, Step)], to: f64) {
    engine.restart();
    for (at, step) in inputs {
        if *at > to {
            break;
        }
        engine.advance_to(*at);
        match step {
            Step::Trigger { trigger } => engine.trigger(trigger),
            Step::Set { set } => {
                for (name, value) in set {
                    engine.set_variable(name, value.clone());
                }
            }
            _ => {}
        }
    }
    engine.advance_to(to);
}

fn main() {
    let shows: Vec<String> = std::env::args().skip(1).collect();
    if shows.is_empty() {
        eprintln!("usage: seek_timing <show> [show...]");
        std::process::exit(2);
    }
    for path in shows {
        let mut engine = Engine::new();
        let loaded = load(&mut engine, &path).expect("the show loads");
        let name = engine.show().map(|s| s.name.clone()).unwrap_or_default();
        let json = std::fs::read_to_string(loaded.show.clone()).expect("the document reads");
        let driver = loaded.driver.clone().unwrap_or(Driver {
            looping: false,
            steps: Vec::new(),
        });
        let one_pass: f64 = driver
            .steps
            .iter()
            .map(|s| {
                if let Step::Wait { wait } = s {
                    *wait
                } else {
                    0.0
                }
            })
            .sum();
        println!(
            "\n== {name}: driver {} step(s), one pass {one_pass:.1} s{}",
            driver.steps.len(),
            if driver.looping { ", looping" } else { "" }
        );

        // A reload: what every edit costs before anything replays.
        let started = Instant::now();
        engine.load_show(&json).expect("the document loads");
        println!(
            "reload (parse + load_show): {:.2} ms",
            started.elapsed().as_secs_f64() * 1e3
        );

        let live = Live::default();
        let targets = [
            1.0,
            5.0,
            20.0,
            one_pass * 0.6,
            one_pass,
            one_pass * 1.5,
            300.0,
        ];
        println!(
            "{:>9}  {:>10}  {:>10}  {:>6}  same frame",
            "to (s)", "60 fps", "jumps", "inputs"
        );
        for to in targets {
            let all = inputs(&driver, to);
            let started = Instant::now();
            seek(engine.core_mut(), Some(driver.clone()), &live, to, 60.0);
            let stepped = started.elapsed().as_secs_f64() * 1e3;
            let a: BTreeMap<_, _> = engine
                .values()
                .unwrap()
                .into_iter()
                .map(|(n, p, v)| ((n, format!("{p:?}")), v))
                .collect();

            let started = Instant::now();
            jump(&mut engine, &all, to);
            let jumped = started.elapsed().as_secs_f64() * 1e3;
            let b: BTreeMap<_, _> = engine
                .values()
                .unwrap()
                .into_iter()
                .map(|(n, p, v)| ((n, format!("{p:?}")), v))
                .collect();
            let differ = a.iter().filter(|(k, v)| b.get(*k) != Some(v)).count();
            println!(
                "{to:>9.1}  {stepped:>7.2} ms  {jumped:>7.2} ms  {:>6}  {}",
                all.iter().filter(|(at, _)| *at <= to).count(),
                if differ == 0 {
                    "yes".to_owned()
                } else {
                    format!("no, {differ} of {} values differ", a.len())
                }
            );
        }
    }
}
