// GUI no Windows: sem janela de console atrás do app.
#![windows_subsystem = "windows"]

mod app;
mod askpass;
mod avatars;
mod git;
mod hosting;
mod icons;
mod jobs;
mod repo_hosting;
mod repo_tab;
mod settings;
mod theme;
mod update;
mod ui;
mod wsl;

use std::path::PathBuf;

fn main() -> eframe::Result {
    // Invocado pelo git/ssh como askpass: responde e sai sem abrir janela.
    if let Ok(sock) = std::env::var(askpass::SOCKET_ENV) {
        std::process::exit(askpass::helper_main(&sock));
    }
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).filter(|p| p.exists()).collect();
    // libgit2 recusa repositórios cujo dono "parece" outro usuário; caminhos
    // \\wsl$ sempre parecem. Só leituras passam pelo libgit2 (nada executa
    // hooks ou config do repo); escrita e rede vão pelo `git` CLI.
    #[cfg(windows)]
    unsafe {
        let _ = git2::opts::set_verify_owner_validation(false);
    }
    update::cleanup();
    let saved = settings::Settings::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("brio-git")
            .with_decorations(!saved.custom_titlebar)
            .with_app_id("brio-git")
            .with_inner_size(saved.layout.window.unwrap_or([1400.0, 900.0]))
            .with_maximized(saved.layout.maximized)
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("brio-git", options, Box::new(move |cc| Ok(Box::new(app::BrioApp::new(cc, args)))))
}
