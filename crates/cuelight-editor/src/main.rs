use cuelight_editor::app::App;

pub fn main() -> iced::Result {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use clap::Parser;
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
        let _ = cuelight_editor::app::OPTIONS.set(cuelight_editor::app::Options::parse());
    }
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
    }

    iced::application(App::new, App::update, App::view)
        // The UI's fonts travel with it: a browser has none of its own to
        // offer, and the desktop then looks the same.
        .fonts([
            include_bytes!("../fonts/AtkinsonHyperlegible-Regular.ttf").as_slice(),
            include_bytes!("../fonts/DMMono-Regular.ttf").as_slice(),
        ])
        .font(iced::Font::new("Atkinson Hyperlegible"))
        .title(App::title)
        .theme(cuelight_editor::app::window_theme)
        // Closing with unsaved edits asks first.
        .exit_on_close_request(false)
        .subscription(App::subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(1100.0, 700.0),
            #[cfg(not(target_arch = "wasm32"))]
            icon: window_icon(),
            ..iced::window::Settings::default()
        })
        .run()
}

/// The logo, the same as the web page's icon, drawn for the window.
#[cfg(not(target_arch = "wasm32"))]
fn window_icon() -> Option<iced::window::Icon> {
    use resvg::{tiny_skia, usvg};
    const SIDE: u32 = 64;
    let logo = include_bytes!("../../../web/favicon.svg");
    let tree = usvg::Tree::from_data(logo, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(SIDE, SIDE)?;
    let scale = SIDE as f32 / tree.size().width();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // The logo is opaque throughout, so its premultiplied pixels are
    // the plain ones the icon takes.
    iced::window::icon::from_rgba(pixmap.take(), SIDE, SIDE).ok()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn the_logo_draws_as_the_window_icon() {
        assert!(super::window_icon().is_some());
    }
}
