//! Execução de operações demoradas fora da thread de UI.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

pub struct JobResult {
    pub label: String,
    pub result: anyhow::Result<String>,
    /// Ação de desfazer registrada se a operação deu certo.
    pub undo: Option<(crate::repo_tab::UndoEntry, crate::repo_tab::UndoStack)>,
}

pub struct Jobs {
    tx: Sender<JobResult>,
    rx: Receiver<JobResult>,
    pub running: Vec<(String, Instant)>,
    ctx: egui::Context,
}

impl Jobs {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, running: Vec::new(), ctx }
    }

    pub fn spawn(
        &mut self,
        label: impl Into<String>,
        undo: Option<(crate::repo_tab::UndoEntry, crate::repo_tab::UndoStack)>,
        f: impl FnOnce() -> anyhow::Result<String> + Send + 'static,
    ) {
        let label = label.into();
        self.running.push((label.clone(), Instant::now()));
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = f();
            let _ = tx.send(JobResult { label, result, undo });
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self) -> Vec<JobResult> {
        let done: Vec<JobResult> = self.rx.try_iter().collect();
        for d in &done {
            if let Some(i) = self.running.iter().position(|(l, _)| *l == d.label) {
                self.running.remove(i);
            }
        }
        done
    }

    pub fn busy(&self) -> bool {
        !self.running.is_empty()
    }
}
