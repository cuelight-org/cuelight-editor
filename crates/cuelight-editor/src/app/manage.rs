//! Managing the show's assets from the window: files imported by a
//! dialog or a drop, an asset put on the stage, and one replaced,
//! renamed, deleted or located on disk.
//!
//! The edits are the core's ([`manage`]): a file change goes into the
//! document's undo history with the JSON edits it causes. Whatever
//! changed the files (an edit, an undo, a redo), the engine follows:
//! the show is loaded again from its files as they stand, into the
//! session at the playhead, so the next frame draws what they say.

use std::collections::BTreeMap;

use cuelight_editor_core::assets::Kind;
use cuelight_editor_core::document::Bytes;
use cuelight_editor_core::manage;
use iced::widget::Widget as _;
use iced::widget::{button, column, row, space, text, text_input};
use iced::{Element, Fill, Task};

use super::{App, Message, Tab, faces, inspector, load_fonts, thumbs};
use crate::dialog;
use cuelight_editor_core::opened::Opened;
use cuelight_editor_core::save;
use cuelight_editor_core::session::lock;

/// The document's edits of asset files, by path: what the engine is
/// registered from besides the files as shipped.
pub(super) fn asset_edits(
    files: &BTreeMap<String, Option<Bytes>>,
) -> BTreeMap<String, Option<Bytes>> {
    files
        .iter()
        .filter(|(path, _)| path.starts_with("assets/"))
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect()
}

impl App {
    /// Register the show's assets again when the document's files are
    /// not what the engine has.
    pub(super) fn follow_files(&mut self) -> Task<Message> {
        let changed = self
            .document
            .as_ref()
            .is_some_and(|d| asset_edits(d.files()) != self.applied_files);
        if changed {
            self.refresh_assets()
        } else {
            Task::none()
        }
    }

    /// Load the show again from its files as they stand, swap the
    /// engine for it under the session, and put it back at the
    /// playhead; the library follows. The picked asset stays picked
    /// while the show has it.
    fn refresh_assets(&mut self) -> Task<Message> {
        let (Some(document), Some(origin)) = (&self.document, &self.origin) else {
            return Task::none();
        };
        self.applied_files = asset_edits(document.files());
        let text = document.text();
        let files = save::files_with(&self.files, document);
        let opened = match Opened::from_files(&self.source, origin.clone(), files) {
            Ok(opened) => opened,
            Err(error) => {
                self.status = format!("the show's assets do not load: {error}");
                return Task::none();
            }
        };
        let Opened {
            engine,
            summary,
            sound_files,
            sounds,
            library,
            files,
            ..
        } = opened;
        let picked = self
            .selected
            .and_then(|i| self.library.get(i))
            .map(|a| (a.name.clone(), a.kind));
        self.thumbs = thumbs(&engine, &library);
        self.faces = faces(&engine, &files, &library);
        let fonts = load_fonts(&engine, &library);
        self.selected = picked.and_then(|(name, kind)| {
            library
                .iter()
                .position(|a| a.name == name && a.kind == kind)
        });
        self.library = library;
        self.preview = None;
        self.summary = summary;
        let heard = self.listen(&sounds, sound_files);
        if let Some(session) = &mut self.session {
            *lock(&session.engine) = engine;
            session.files = files.keys().cloned().collect();
        }
        if let Err(error) = self.reload_text(&text, true) {
            self.status = error;
        }
        Task::batch([heard, fonts])
    }

    /// Import files into the show's assets, as one step, and pick the
    /// first in the library.
    pub(super) fn import(&mut self, picked: Vec<dialog::File>) -> Task<Message> {
        let Some(document) = &mut self.document else {
            self.status = "open a show to import into".to_owned();
            return Task::none();
        };
        let imports = manage::import(document, &self.files, &picked);
        let task = self.follow_files();
        let names: Vec<&str> = imports
            .imported
            .iter()
            .map(|(name, _, _)| name.as_str())
            .collect();
        let mut said = if names.is_empty() {
            "imported nothing".to_owned()
        } else {
            format!("imported {}", names.join(", "))
        };
        for refused in &imports.refused {
            said = format!("{said}; left out {refused}");
        }
        self.status = said;
        if let Some((name, kind, _)) = imports.imported.first() {
            self.tab = Tab::Assets;
            self.style = None;
            self.asset_name_typed = None;
            self.selected = self
                .library
                .iter()
                .position(|a| a.name == *name && a.kind == *kind);
        }
        task
    }

    /// A dropped file: imported into the open show when it is a kind
    /// of asset; anything else is a show to open.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn dropped(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let asset = path.is_file()
            && path
                .file_name()
                .is_some_and(|n| manage::place_for(&n.to_string_lossy()).is_some());
        if asset && self.session.is_some() {
            return self.import(dialog::read_assets(&[path]));
        }
        self.open(Opened::from_path(&path))
    }

    /// Put an asset on the stage: a layer for it, in the middle of the
    /// canvas. A font gets a style instead, which text is written in.
    pub(super) fn use_asset(&mut self, index: usize) -> Task<Message> {
        use cuelight_editor_core::layers::Kind as Layer;
        let Some(asset) = self.library.get(index) else {
            return Task::none();
        };
        let mut making = self.making();
        let kind = match asset.kind {
            Kind::Image | Kind::Vector => {
                making.artwork = Some((asset.name.clone(), asset.kind == Kind::Vector, asset.size));
                Layer::Image
            }
            Kind::Sound => {
                making.sound = Some(asset.name.clone());
                Layer::Audio
            }
            Kind::Video => {
                making.video = Some(asset.name.clone());
                Layer::Video
            }
            Kind::Font => return self.add_style(asset.name.clone()),
        };
        self.insert_made(kind, making)
    }

    /// Delete an asset, unless the show uses it: then what uses it is
    /// said instead.
    pub(super) fn delete_asset(&mut self, index: usize) -> Task<Message> {
        let (Some(asset), Some(document)) = (self.library.get(index), &mut self.document) else {
            return Task::none();
        };
        let name = asset.name.clone();
        match manage::delete(document, &self.files, asset) {
            Ok(()) => {
                self.selected = None;
                let task = self.follow_files();
                self.status = format!("deleted {name}");
                task
            }
            Err(why) => {
                self.status = format!("cannot delete: {why}");
                Task::none()
            }
        }
    }

    /// Give the picked asset the name typed for it: its file and every
    /// use, as one step.
    pub(super) fn rename_asset(&mut self) -> Task<Message> {
        let Some(typed) = self.asset_name_typed.take() else {
            return Task::none();
        };
        let (Some(asset), Some(document)) = (
            self.selected.and_then(|i| self.library.get(i)),
            &mut self.document,
        ) else {
            return Task::none();
        };
        let from = asset.name.clone();
        match manage::rename(document, &self.files, asset, &typed) {
            Ok(_) if typed.trim() == from => Task::none(),
            Ok(places) => {
                let task = self.follow_files();
                self.status = format!("renamed {from} to {}, and {places} use(s)", typed.trim());
                task
            }
            Err(why) => {
                self.status = format!("cannot rename {from}: {why}");
                self.asset_name_typed = Some(typed);
                Task::none()
            }
        }
    }

    /// Give the asset `name` the bytes of a file picked for it.
    pub(super) fn replace_asset(&mut self, name: &str, file: dialog::File) -> Task<Message> {
        let (Some(asset), Some(document)) = (
            self.library.iter().find(|a| a.name == name),
            &mut self.document,
        ) else {
            return Task::none();
        };
        let (file_name, bytes) = file;
        match manage::replace(document, &self.files, asset, &file_name, bytes) {
            Ok(()) => {
                let task = self.follow_files();
                self.status = format!("replaced {name} with {file_name}");
                task
            }
            Err(why) => {
                self.status = format!("cannot replace {name}: {why}");
                Task::none()
            }
        }
    }

    /// Show the asset's file in the system's file manager: the folder
    /// it is in, picked where the system can.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn locate_asset(&mut self, index: usize) -> Task<Message> {
        let (Some(asset), Some(super::Origin::Folder(dir))) =
            (self.library.get(index), &self.origin)
        else {
            self.status = "only a show folder has its assets on disk".to_owned();
            return Task::none();
        };
        let Some(path) = asset.file.as_ref().map(|f| dir.join(f)) else {
            return Task::none();
        };
        if !path.is_file() {
            self.status = format!("{} is not on disk until the show is saved", asset.name);
            return Task::none();
        }
        let shown = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
                .arg("-R")
                .arg(&path)
                .spawn()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn()
        } else {
            let folder = path.parent().unwrap_or(dir);
            std::process::Command::new("xdg-open").arg(folder).spawn()
        };
        self.status = match shown {
            Ok(_) => format!("showing {}", path.display()),
            Err(error) => format!("could not show {}: {error}", path.display()),
        };
        Task::none()
    }

    /// What can be done with the picked asset: put on the stage,
    /// replaced, deleted, located; and its name to rename it by.
    pub(super) fn asset_actions<'a>(&'a self, index: usize) -> Element<'a, Message> {
        let Some(asset) = self.library.get(index) else {
            return space::horizontal().width(0).boxed();
        };
        let has_file = asset.file.is_some();
        let small = |label: &'a str, message: Option<Message>| {
            button(text(label).size(13))
                .on_press_maybe(message)
                .style(button::secondary)
                .padding([2, 8])
        };
        let use_label = if asset.kind == Kind::Font {
            "Add style"
        } else {
            "Use"
        };
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut actions = row![
            small(use_label, Some(Message::UseAsset(index))),
            small(
                "Replace...",
                (has_file && !self.asking).then_some(Message::ReplaceAsset(index))
            ),
            small("Delete", has_file.then_some(Message::DeleteAsset(index))),
        ]
        .spacing(6);
        #[cfg(not(target_arch = "wasm32"))]
        {
            actions = actions.push(
                small(
                    "Locate",
                    (has_file && matches!(self.origin, Some(super::Origin::Folder(_))))
                        .then_some(Message::LocateAsset(index)),
                )
                .boxed(),
            );
        }
        let typed = self.asset_name_typed.as_deref();
        let mut name = text_input(&asset.name, typed.unwrap_or(&asset.name))
            .size(13)
            .padding([1, 4])
            .width(Fill)
            .style(inspector::field_style(typed.is_some(), false));
        if has_file {
            name = name
                .on_input(Message::TypeAssetName)
                .on_submit(Message::ApplyAssetName);
        }
        column![
            row![text("name").size(13).width(86), name]
                .spacing(6)
                .align_y(iced::Center),
            actions.wrap().vertical_spacing(6),
        ]
        .spacing(6)
        .boxed()
    }
}
