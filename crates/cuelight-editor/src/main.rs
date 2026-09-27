use cuelight_editor::app::App;

pub fn main() -> iced::Result {
    #[cfg(not(target_arch = "wasm32"))]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
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
        .subscription(App::subscription)
        .window_size((1100.0, 700.0))
        .run()
}
