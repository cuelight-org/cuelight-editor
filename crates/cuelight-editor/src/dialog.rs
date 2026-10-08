//! The open dialogs, native and web, and the download that saves in a
//! browser.
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

/// Ask for a show file: a packed show, or on the desktop a loose show
/// document too. A browser hands over one file without its folder, so
/// there only a pack brings its assets along.
pub async fn pick_file() -> Option<Picked> {
    #[cfg(not(target_arch = "wasm32"))]
    let extensions: &[&str] = &["cuelight", "json"];
    #[cfg(target_arch = "wasm32")]
    let extensions: &[&str] = &["cuelight"];
    let picked = rfd::AsyncFileDialog::new()
        .set_title("Open a show")
        .add_filter("cuelight show", extensions)
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

/// In a browser, the show the page was asked to open: `?show=<url>`, a
/// packed show or a loose document, fetched as bytes. `None` when the
/// page was opened plainly.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_show_from_query() -> Option<Picked> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    let url = web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get("show")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(&url))
        .await
        .ok()?
        .dyn_into()
        .ok()?;
    if !response.ok() {
        log::warn!("could not fetch {url}: status {}", response.status());
        return None;
    }
    let buffer = JsFuture::from(response.array_buffer().ok()?).await.ok()?;
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
    let name = url.rsplit('/').next().unwrap_or("show.cuelight").to_owned();
    Some(Picked::File { name, bytes })
}

/// In a browser, the files dropped on the page, as they come. The page
/// itself has to accept drops (a browser opens a dropped file otherwise),
/// so the listeners go on the document and stay for the page's life.
#[cfg(target_arch = "wasm32")]
pub fn drops() -> impl iced::futures::Stream<Item = Picked> {
    use iced::futures::channel::mpsc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen_futures::JsFuture;

    let (sender, receiver) = mpsc::unbounded();
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        let allow = Closure::<dyn Fn(web_sys::Event)>::new(|event: web_sys::Event| {
            event.prevent_default();
        });
        let _ =
            document.add_event_listener_with_callback("dragover", allow.as_ref().unchecked_ref());
        allow.forget();

        let dropped =
            Closure::<dyn Fn(web_sys::DragEvent)>::new(move |event: web_sys::DragEvent| {
                event.prevent_default();
                let Some(file) = event
                    .data_transfer()
                    .and_then(|transfer| transfer.files())
                    .and_then(|files| files.get(0))
                else {
                    return;
                };
                let sender = sender.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let name = file.name();
                    let Ok(buffer) = JsFuture::from(file.array_buffer()).await else {
                        log::warn!("could not read the dropped file {name}");
                        return;
                    };
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    let _ = sender.unbounded_send(Picked::File { name, bytes });
                });
            });
        let _ = document.add_event_listener_with_callback("drop", dropped.as_ref().unchecked_ref());
        dropped.forget();
    }
    receiver
}

/// In a browser, hand `bytes` to the person as a download named `name`:
/// a page cannot write a file back where it came from, so this is what
/// saving is there.
#[cfg(target_arch = "wasm32")]
pub fn offer_download(name: &str, bytes: &[u8]) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let said = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(said)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(said)?;
    let link: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(said)?
        .dyn_into()
        .map_err(|_| "not a link".to_owned())?;
    link.set_href(&url);
    link.set_download(name);
    link.click();
    web_sys::Url::revoke_object_url(&url).map_err(said)
}

/// Ask where to write a file, named `name` in `folder` to begin with,
/// of the kind `filter` names with its extensions. Desktop only: a
/// browser downloads instead.
#[cfg(not(target_arch = "wasm32"))]
pub async fn save_to(
    title: &'static str,
    name: String,
    folder: Option<std::path::PathBuf>,
    filter: (&'static str, &'static [&'static str]),
) -> Option<std::path::PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title(title)
        .set_file_name(name)
        .add_filter(filter.0, filter.1);
    if let Some(folder) = folder {
        dialog = dialog.set_directory(folder);
    }
    Some(dialog.save_file().await?.path().to_owned())
}
