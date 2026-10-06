//! The open show changed on disk (desktop only): a change from outside
//! the editor reloads it at the playhead, unless it has unsaved edits,
//! which are not dropped without asking.

use super::*;
use cuelight_editor_core::watch::Change;

impl App {
    /// Paths under the watch changed: reload if the show's files are not
    /// what the editor last opened or saved. The editor's own save
    /// leaves them the same, so it reloads nothing.
    pub(super) fn disk_changed(&mut self, paths: &[std::path::PathBuf]) -> Task<Message> {
        let Some(origin) = &self.origin else {
            return Task::none();
        };
        if !paths.iter().any(|path| watch::concerns(origin, path)) {
            return Task::none();
        }
        match watch::change(origin, &self.files) {
            Ok(None) => Task::none(),
            Ok(Some(change)) if self.document.as_ref().is_some_and(Document::is_dirty) => {
                self.status = "the show changed on disk, and you have unsaved edits".to_owned();
                self.outside = Some(change);
                Task::none()
            }
            Ok(Some(change)) => self.reload(change),
            Err(error) => {
                self.status = format!("the show changed on disk and could not be read: {error}");
                log::warn!("{}", self.status);
                Task::none()
            }
        }
    }

    /// Take the show as it is on disk and put it back at the playhead.
    /// A new document alone reloads into the session, its inputs
    /// replayed; a new driver or asset opens the show again. Either way
    /// the undo history is gone, since it undoes into the old text.
    pub(super) fn reload(&mut self, change: Change) -> Task<Message> {
        self.outside = None;
        let history = self
            .document
            .as_ref()
            .is_some_and(|d| d.can_undo() || d.can_redo());
        let what = if change.others.is_empty() {
            "show.json".to_owned()
        } else {
            let mut names = change.others.clone();
            if change.document {
                names.insert(0, "show.json".to_owned());
            }
            names.join(", ")
        };
        let reloaded = if change.others.is_empty() {
            self.reload_document(change).map(|()| Task::none())
        } else {
            self.reopen()
        };
        let task = match reloaded {
            Ok(task) => task,
            Err(error) => {
                self.status = error;
                log::warn!("{}", self.status);
                return Task::none();
            }
        };
        self.status = if history {
            format!("reloaded {what} from disk; the undo history is cleared")
        } else {
            format!("reloaded {what} from disk")
        };
        log::info!("{}", self.status);
        task
    }

    /// The show document changed on disk and nothing else did: load it
    /// into the playing session. Text that is no document keeps the show
    /// as it was.
    fn reload_document(&mut self, change: Change) -> Result<(), String> {
        let text = change
            .files
            .get("show.json")
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .ok_or("show.json is gone from disk")?;
        let document = Document::parse(&text).map_err(|e| {
            format!("show.json on disk does not read ({e}); still showing the last version")
        })?;
        self.reload_text(&text, true)?;
        self.document = Some(document);
        self.files = change.files;
        Ok(())
    }

    /// A driver or an asset changed on disk: open the show again and go
    /// back to where it was.
    fn reopen(&mut self) -> Result<Task<Message>, String> {
        let Some(path) = self
            .origin
            .as_ref()
            .and_then(watch::path)
            .map(ToOwned::to_owned)
        else {
            return Ok(Task::none());
        };
        let opened = Opened::from_path(&path).map_err(|e| {
            format!("the show on disk does not open ({e}); still showing the last version")
        })?;
        let (time, paused) = self
            .session
            .as_ref()
            .map_or((0.0, true), |s| (s.time, s.paused));
        let task = self.open(Ok(opened));
        if let Some(session) = &mut self.session {
            session.seek(time, Instant::now());
            session.paused = paused;
        }
        Ok(task)
    }

    /// The question a change on disk asks while there are unsaved edits.
    pub(super) fn outside_prompt(&self) -> Option<Element<'_, Message>> {
        self.outside.as_ref()?;
        Some(
            container(
                row![
                    text("The show changed on disk, and you have unsaved edits.").size(13),
                    button("Keep my edits").on_press(Message::KeepEdits),
                    button("Load from disk").on_press(Message::LoadFromDisk),
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .padding([4, 8])
            .width(Fill)
            .boxed(),
        )
    }
}
