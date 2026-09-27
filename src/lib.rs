//! The cuelight editor: opens a show, plays it on a stage, and says what
//! it found.
//!
//! One program for the desktop and the browser. The desktop opens show
//! folders, packed shows (`.cuelight`) and loose show files by dialog or
//! by dropping them on the window; the browser opens a packed or loose
//! show through its file picker.

pub mod app;
pub mod dialog;
pub mod opened;
pub mod session;
pub mod stage;
