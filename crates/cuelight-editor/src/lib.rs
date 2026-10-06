//! The cuelight editor's window: the stage, the panels and the dialogs,
//! over what `cuelight-editor-core` knows about a show.

pub mod app;
pub mod dialog;
pub mod stage;
#[cfg(not(target_arch = "wasm32"))]
pub mod watcher;
