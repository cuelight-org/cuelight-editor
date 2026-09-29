//! What the cuelight editor knows without a window: opening a show in
//! any of its forms, and playing it on a clock. No iced, no GPU, so it
//! compiles in seconds and its tests run anywhere, the browser included.

pub mod assets;
pub mod document;
pub mod inputs;
pub mod log;
pub mod opened;
pub mod session;
pub mod tree;
