//! Probe de desenvolvimento: verifica se o compositor atende os comandos de
//! janela usados pela barra de título. `cargo run --example window_probe`
use std::time::{Duration, Instant};

struct Probe {
    start: Instant,
    step: usize,
}

const PLAN: [(f32, &str); 6] = [(1.5, "maximize"), (3.0, "restore"), (4.5, "fullscreen"), (6.0, "exit-fullscreen"), (7.5, "minimize"), (9.5, "close")];

impl eframe::App for Probe {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t = self.start.elapsed().as_secs_f32();
        let info = ctx.input(|i| (i.viewport().maximized, i.viewport().fullscreen, i.viewport().minimized, i.viewport().inner_rect.map(|r| r.size())));
        ui.label(format!("t={t:.1} {info:?}"));
        if let Some((at, what)) = PLAN.get(self.step)
            && t >= *at
        {
            println!("t={t:.1} antes de {what}: maximized={:?} fullscreen={:?} size={:?}", info.0, info.1, info.3);
            use egui::ViewportCommand as V;
            ctx.send_viewport_cmd(match *what {
                "maximize" => V::Maximized(true),
                "restore" => V::Maximized(false),
                "fullscreen" => V::Fullscreen(true),
                "exit-fullscreen" => V::Fullscreen(false),
                "minimize" => V::Minimized(true),
                _ => V::Close,
            });
            self.step += 1;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("brio-git-window-probe").with_app_id("brio-git-window-probe").with_decorations(false).with_inner_size([700.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native("probe", options, Box::new(|_| Ok(Box::new(Probe { start: Instant::now(), step: 0 }))))
}
