//! Watching the open show on disk (desktop only): the paths that
//! changed, in bursts. A save writes several files, an editor writes a
//! file in steps; a burst is what changed in a quiet moment after them.
//! Which of the paths are the show's, and whether its files really
//! differ from what the editor holds, is for the app to tell.

use std::path::PathBuf;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::{Stream, StreamExt};
use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};

/// How long the files have to stay still before a burst is reported.
const QUIET: Duration = Duration::from_millis(150);

/// The bursts of changed paths under `at`, under its folders too when
/// the flag says so. A folder that cannot be watched gives nothing.
pub fn watch((at, under): &(PathBuf, bool)) -> impl Stream<Item = Vec<PathBuf>> + use<> {
    let (sender, receiver) = mpsc::unbounded();
    let debouncer = new_debouncer(QUIET, move |result: DebounceEventResult| match result {
        Ok(events) if !events.is_empty() => {
            let _ = sender.unbounded_send(events.into_iter().map(|e| e.path).collect());
        }
        Ok(_) => {}
        Err(error) => log::warn!("watching the show: {error}"),
    });
    let debouncer = match debouncer {
        Ok(mut debouncer) => {
            let mode = if *under {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            match debouncer.watcher().watch(at, mode) {
                Ok(()) => Some(debouncer),
                Err(error) => {
                    log::warn!("cannot watch {}: {error}", at.display());
                    None
                }
            }
        }
        Err(error) => {
            log::warn!("cannot watch {}: {error}", at.display());
            None
        }
    };
    // The stream owns the watch: it stops when the subscription does.
    receiver.map(move |paths| {
        let _watching = &debouncer;
        paths
    })
}
