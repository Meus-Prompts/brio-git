// Release builds on Windows are a GUI app: no console window next to it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod app;
mod avatars;
mod branch_picker;
mod data;
mod fileview;
mod highlight;
mod history;
mod icons;
mod keys;
mod layout;
mod menu;
mod persist;
mod session;
mod style;
mod toasts;
mod update;
mod util;
mod view;
mod view_details;
mod view_file;
mod view_overlay;
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
        .font(icons::FONT_BYTES)
        .window(iced::window::Settings {
            size: iced::Size::new(1440.0, 920.0),
            min_size: Some(iced::Size::new(720.0, 480.0)),
            decorations: false,
            #[cfg(target_os = "linux")]
            platform_specific: iced::window::settings::PlatformSpecific {
                // Matches StartupWMClass in packaging/brio-git.desktop.
                application_id: "brio-git".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .exit_on_close_request(false)
        .run()
}
