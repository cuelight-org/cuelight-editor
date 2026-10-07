//! Unsaved edits kept in the journal as they are made, and offered back
//! when the show is opened again after a crash or a closed tab. A
//! browser also asks before a tab with unsaved edits is closed.

use super::*;
use cuelight_editor_core::journal::{self, Entry, Found};

/// The open show's journal.
#[derive(Debug, Default)]
pub(super) struct Journal {
    /// The folder entries are kept in (desktop only); `None` keeps no
    /// journal, which is how tests stay out of the person's own.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) folder: Option<std::path::PathBuf>,
    /// The open show's key.
    show: Option<String>,
    /// The document's revision, and whether it had unsaved edits, as
    /// last written: what tells an edit from a frame.
    written: Option<(u64, bool)>,
    /// What the journal held when the show was opened, until it is
    /// restored or discarded. The journal is left alone meanwhile.
    pub(super) offer: Option<Found>,
}

impl Journal {
    /// The journal kept where this editor keeps it.
    pub(super) fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        ask_before_closing();
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            folder: if cfg!(test) { None } else { journal::folder() },
            ..Self::default()
        }
    }

    fn read(&self, show: &str) -> Option<Entry> {
        #[cfg(not(target_arch = "wasm32"))]
        return journal::read(self.folder.as_deref()?, show);
        #[cfg(target_arch = "wasm32")]
        return storage()?
            .get_item(&storage_key(show))
            .ok()
            .flatten()
            .and_then(|text| Entry::from_json(&text))
            .filter(|entry| entry.show == show);
    }

    fn write(&self, entry: &Entry) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        return match &self.folder {
            Some(folder) => journal::write(folder, entry),
            None => Ok(()),
        };
        #[cfg(target_arch = "wasm32")]
        return storage()
            .ok_or("the browser keeps no storage for this page")?
            .set_item(&storage_key(&entry.show), &entry.to_json())
            .map_err(|e| format!("{e:?}"));
    }

    fn clear(&self, show: &str) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        return match &self.folder {
            Some(folder) => journal::clear(folder, show),
            None => Ok(()),
        };
        #[cfg(target_arch = "wasm32")]
        return match storage() {
            Some(storage) => storage
                .remove_item(&storage_key(show))
                .map_err(|e| format!("{e:?}")),
            None => Ok(()),
        };
    }
}

impl App {
    /// A show was opened: what its journal holds is offered back.
    pub(super) fn journal_opened(&mut self) {
        let Some(origin) = &self.origin else {
            return;
        };
        let show = journal::key(origin);
        let opened = self.files.get("show.json").map_or(&[][..], Vec::as_slice);
        self.journal.offer = journal::found(self.journal.read(&show), opened);
        self.journal.show = Some(show);
        self.journal.written = None;
    }

    /// The document was replaced by another, not edited: what is
    /// written next is about the new one. (A reload from disk, desktop
    /// only.)
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn journal_replaced(&mut self) {
        self.journal.written = None;
    }

    /// After every message: an edit (or undo, or redo) writes the
    /// document to the journal, and a show with no unsaved edits left
    /// takes its entry away. A drag writes when it lets go.
    pub(super) fn keep_journal(&mut self) {
        #[cfg(target_arch = "wasm32")]
        UNSAVED.with(|unsaved| unsaved.set(self.document.as_ref().is_some_and(Document::is_dirty)));
        if self.journal.offer.is_some() {
            return;
        }
        let (Some(show), Some(document)) = (&self.journal.show, &self.document) else {
            return;
        };
        if document.in_step() {
            return;
        }
        let now = (document.revision(), document.is_dirty());
        if self.journal.written == Some(now) {
            return;
        }
        self.journal.written = Some(now);
        let result = if now.1 {
            self.journal.write(&Entry {
                show: show.clone(),
                base: journal::base(self.files.get("show.json").map_or(&[][..], Vec::as_slice)),
                at: journal::now(),
                text: document.text(),
            })
        } else {
            self.journal.clear(show)
        };
        if let Err(error) = result {
            log::warn!("the journal could not be kept: {error}");
        }
    }

    /// Put the journal's edits back, as one step to undo.
    pub(super) fn restore_edits(&mut self) -> Task<Message> {
        let (Some(found), Some(document)) = (self.journal.offer.take(), &mut self.document) else {
            return Task::none();
        };
        if let Err(error) = document.replace(&found.entry.text) {
            self.forget_edits();
            self.status = format!("the unsaved edits do not read ({error}); discarded them");
            return Task::none();
        }
        let text = document.text();
        match self.reload_text(&text, true) {
            Ok(()) => {
                self.status = "restored the unsaved edits; undo takes them back".to_owned();
            }
            Err(error) => {
                // A show that does not load still has its text put back:
                // it is the person's work, and the next edit may fix it.
                self.status = format!("restored the unsaved edits, but {error}");
            }
        }
        log::info!("{}", self.status);
        Task::none()
    }

    /// Drop the journal's edits; the show stays as opened.
    pub(super) fn discard_edits(&mut self) -> Task<Message> {
        self.forget_edits();
        self.status = "discarded the unsaved edits".to_owned();
        Task::none()
    }

    /// Take the open show's entry away, and what it offered.
    pub(super) fn forget_edits(&mut self) {
        self.journal.offer = None;
        if let Some(show) = &self.journal.show
            && let Err(error) = self.journal.clear(show)
        {
            log::warn!("the journal could not be cleared: {error}");
        }
    }

    /// The question an opened show with unsaved edits in its journal
    /// asks.
    pub(super) fn restore_prompt(&self) -> Option<Element<'_, Message>> {
        let found = self.journal.offer.as_ref()?;
        let ago = journal::ago(found.entry.at, journal::now());
        let said = if found.changed {
            format!(
                "This show has unsaved edits from {ago}, but it changed since; restoring them drops that change."
            )
        } else {
            format!("This show has unsaved edits from {ago}.")
        };
        // In the theme's warning colour, faint behind the words and full
        // round them: it asks for a decision without shouting over the
        // stage, on a light theme or a dark one.
        let bar = container(
            row![
                text(said).size(13).width(Fill),
                button(text("Restore").size(13)).on_press(Message::RestoreEdits),
                button(text("Discard").size(13))
                    .on_press(Message::DiscardEdits)
                    .style(button::secondary),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding([6, 10])
        .width(Fill)
        .style(|theme: &Theme| {
            let warning = theme.palette().warning.base.color;
            container::Style {
                background: Some(iced::Background::Color(warning.scale_alpha(0.18))),
                border: iced::Border {
                    color: warning,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            }
        });
        Some(container(bar).padding([4, 8]).width(Fill).boxed())
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// Whether the open show has unsaved edits, for the page to ask
    /// before it is closed.
    static UNSAVED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The page's own storage. Shows are small enough for it.
#[cfg(target_arch = "wasm32")]
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[cfg(target_arch = "wasm32")]
fn storage_key(show: &str) -> String {
    format!("cuelight-editor/journal/{show}")
}

/// Have the browser ask before the page goes while there are unsaved
/// edits.
#[cfg(target_arch = "wasm32")]
fn ask_before_closing() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let ask =
        Closure::<dyn Fn(web_sys::BeforeUnloadEvent)>::new(|event: web_sys::BeforeUnloadEvent| {
            if UNSAVED.with(std::cell::Cell::get) {
                event.prevent_default();
                event.set_return_value("unsaved edits");
            }
        });
    let _ = window.add_event_listener_with_callback("beforeunload", ask.as_ref().unchecked_ref());
    ask.forget();
}
