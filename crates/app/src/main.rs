mod app;
mod avatars;
mod data;
mod fileview;
mod highlight;
mod keys;
mod layout;
mod persist;
mod style;
mod util;
mod view;
mod view_details;
mod view_file;
mod widgets;
mod workspace;

use std::path::PathBuf;

use app::App;

fn main() -> iced::Result {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let state_file = persist::default_path();

    iced::application(move || App::boot(paths.clone(), state_file.clone()), App::update, view::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .window(iced::window::Settings {
            size: iced::Size::new(1440.0, 920.0),
            min_size: Some(iced::Size::new(720.0, 480.0)),
            decorations: false,
            ..Default::default()
        })
        .exit_on_close_request(false)
        .run()
}
