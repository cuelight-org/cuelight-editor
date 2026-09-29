//! The log below the stage: one line per thing worth knowing, newest
//! last. What the audit says of the document, what the load had to drop,
//! what the engine traced while the show played, and what the hand and
//! the driver told it. Each line says which of those it is, and the ones
//! about the document say where in it.

use std::collections::VecDeque;

use cuelight_core::{Finding, FindingKind, Traced, Value};
use cuelight_loader::{Driver, Step};

/// How many lines are kept; the oldest go first.
pub const CAP: usize = 4000;

/// What sort of line it is: the first word of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The audit of the document, by the finding's kind.
    Audit(FindingKind),
    /// What the load dropped or could not read.
    Load,
    /// What the engine traced as the show played.
    Trace,
    /// A trigger fired, a variable set or a press made by hand.
    Input,
    /// A step of the driver script, at its own instant.
    Driver,
}

impl Kind {
    /// The word the line starts with.
    pub fn label(self) -> String {
        match self {
            Kind::Audit(kind) => format!("audit {}", kind.name()),
            Kind::Load => "load".to_owned(),
            Kind::Trace => "trace".to_owned(),
            Kind::Input => "input".to_owned(),
            Kind::Driver => "driver".to_owned(),
        }
    }
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub kind: Kind,
    /// The show's time it happened at, for what happens while it plays;
    /// a line about the document has none.
    pub at: Option<f64>,
    pub text: String,
}

impl Line {
    /// The line as the log shows it, in columns: the kind, the instant
    /// when there is one, the text.
    pub fn render(&self) -> String {
        let at = match self.at {
            Some(at) => format!("{at:7.2}"),
            None => " ".repeat(7),
        };
        format!("{:<13} {at}  {}", self.kind.label(), self.text)
    }
}

/// The lines, oldest first, at most [`CAP`] of them.
#[derive(Debug, Default)]
pub struct Log {
    lines: VecDeque<Line>,
}

impl Log {
    pub fn push(&mut self, line: Line) {
        self.lines.push_back(line);
        while self.lines.len() > CAP {
            self.lines.pop_front();
        }
    }

    pub fn extend(&mut self, lines: impl IntoIterator<Item = Line>) {
        for line in lines {
            self.push(line);
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Drop the audit's lines, for an audit run again on a changed
    /// document; the rest stays.
    pub fn clear_audit(&mut self) {
        self.lines.retain(|l| !matches!(l.kind, Kind::Audit(_)));
    }

    /// What the audit says of one place: a file's path in the show, or a
    /// path in the document.
    pub fn audit_of<'a>(&'a self, place: &str) -> impl Iterator<Item = &'a Line> {
        let prefix = format!("{place}: ");
        self.lines
            .iter()
            .filter(move |l| matches!(l.kind, Kind::Audit(_)) && l.text.starts_with(&prefix))
    }

    /// Drop what happened while the show played; what is said of the
    /// document stays. For a restart.
    pub fn clear_played(&mut self) {
        self.lines
            .retain(|l| matches!(l.kind, Kind::Audit(_) | Kind::Load));
    }
}

/// The audit of `json` as lines: every finding at the path the document
/// gives it, with `files` the paths of the show folder and `driver` its
/// script, when known.
pub fn audit(json: &str, files: &[String], driver: Option<&Driver>) -> Vec<Line> {
    cuelight_loader::audit(json, Some(files), driver)
        .iter()
        .map(finding)
        .collect()
}

/// One finding as a line: its place, then what is wrong with it.
pub fn finding(finding: &Finding) -> Line {
    Line {
        kind: Kind::Audit(finding.kind),
        at: None,
        text: format!("{}: {}", finding.path, finding.message),
    }
}

/// What the load had to say, one line each.
pub fn load(problems: &[String]) -> Vec<Line> {
    problems
        .iter()
        .map(|problem| Line {
            kind: Kind::Load,
            at: None,
            text: problem.clone(),
        })
        .collect()
}

/// One thing the engine traced, in the trace's own words.
pub fn traced(traced: &Traced) -> Line {
    Line {
        kind: Kind::Trace,
        at: Some(traced.at),
        text: traced.what.to_string(),
    }
}

/// A trigger fired by hand, by a key or by a press.
pub fn fired(at: f64, trigger: &str) -> Line {
    Line {
        kind: Kind::Input,
        at: Some(at),
        text: format!("fired {trigger:?}"),
    }
}

/// A variable set by hand.
pub fn set(at: f64, name: &str, value: &Value) -> Line {
    Line {
        kind: Kind::Input,
        at: Some(at),
        text: format!("set {name:?} to {}", value.to_text()),
    }
}

/// A press that asks for a web address to be opened; the editor only
/// says so.
pub fn opened(at: f64, url: &str) -> Line {
    Line {
        kind: Kind::Input,
        at: Some(at),
        text: format!("press asks to open {url:?}, which the editor leaves closed"),
    }
}

/// A step of the driver, at its own instant; a wait is no line.
pub fn driver(at: f64, step: &Step) -> Option<Line> {
    let text = match step {
        Step::Trigger { trigger } => format!("fired {trigger:?}"),
        Step::Set { set } => set
            .iter()
            .map(|(name, value)| format!("set {name:?} to {}", value.to_text()))
            .collect::<Vec<_>>()
            .join(", "),
        _ => return None,
    };
    Some(Line {
        kind: Kind::Driver,
        at: Some(at),
        text,
    })
}
