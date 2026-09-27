//! The open dialogs, native and web.
//!
//! On the desktop a dialog gives back a path, and the loader reads the
//! folder, pack or file itself. In a browser there is no path, only the
//! picked file's name and bytes, and no way to pick a folder at all.

/// What a dialog picked.
#[derive(Debug, Clone)]
pub enum Picked {
    #[cfg(not(target_arch = "wasm32"))]
    Path(std::path::PathBuf),
    #[cfg(target_arch = "wasm32")]
    File { name: String, bytes: Vec<u8> },
}

/// Ask for a show file: a packed show or a loose show document.
pub async fn pick_file() -> Option<Picked> {
    let picked = rfd::AsyncFileDialog::new()
        .set_title("Open a show")
        .add_filter("cuelight show", &["cuelight", "json"])
        .pick_file()
        .await?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(Picked::Path(picked.path().to_owned()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let name = picked.file_name();
        let bytes = picked.read().await;
        Some(Picked::File { name, bytes })
    }
}

/// Ask for a show folder. Desktop only: a browser cannot pick one.
#[cfg(not(target_arch = "wasm32"))]
pub async fn pick_folder() -> Option<Picked> {
    let picked = rfd::AsyncFileDialog::new()
        .set_title("Open a show folder")
        .pick_folder()
        .await?;
    Some(Picked::Path(picked.path().to_owned()))
}
