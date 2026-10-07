//! What the cuelight editor knows without a window: opening a show in
//! any of its forms, playing it on a clock, saving it back, and noticing
//! when it changes on disk. No iced, no GPU, so it compiles in seconds
//! and its tests run anywhere, the browser included.

pub mod artwork;
pub mod assets;
pub mod document;
pub mod edit;
pub mod fields;
pub mod inputs;
pub mod journal;
pub mod lists;
pub mod log;
pub mod opened;
pub mod placement;
pub mod save;
pub mod session;
pub mod specimen;
pub mod syntax;
pub mod tree;
#[cfg(not(target_arch = "wasm32"))]
pub mod watch;
