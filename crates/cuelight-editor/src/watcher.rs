//! Watching the open show on disk (desktop only): the paths that
//! changed, in bursts. A save writes several files, an editor writes a
//! file in steps; a burst is what changed in a quiet moment after them.
//! Which of the paths are the show's, and whether its files really
//! differ from what the editor holds, is for the app to tell.

use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::{Stream, StreamExt};
use notify_debouncer_mini::notify::{self, EventKind, RecursiveMode, Watcher as _};

/// How long the files have to stay still before a burst is reported.
const QUIET: Duration = Duration::from_millis(150);

/// The bursts of changed paths under `at`, under its folders too when
/// the flag says so. A folder that cannot be watched gives nothing.
///
/// A file only read is no change: the editor reads the show's files to
/// tell whether they changed, and that read must not look like one.
pub fn watch((at, under): &(PathBuf, bool)) -> impl Stream<Item = Vec<PathBuf>> + use<> {
    let (sender, receiver) = mpsc::unbounded();
    let (events, burst) = std_mpsc::channel::<Vec<PathBuf>>();
    let watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                let _ = events.send(event.paths);
            }
            Ok(_) => {}
            Err(error) => log::warn!("watching the show: {error}"),
        });
    // Gather the paths until the files stay still for a while, then send
    // them as one burst. The thread ends with the watch.
    std::thread::spawn(move || {
        while let Ok(mut paths) = burst.recv() {
            while let Ok(more) = burst.recv_timeout(QUIET) {
                paths.extend(more);
            }
            paths.sort();
            paths.dedup();
            if sender.unbounded_send(paths).is_err() {
                break;
            }
        }
    });
    let watcher = match watcher {
        Ok(mut watcher) => {
            let mode = if *under {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            match watcher.watch(at, mode) {
                Ok(()) => Some(watcher),
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
        let _watching = &watcher;
        paths
    })
}
