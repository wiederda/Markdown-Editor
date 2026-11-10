#![windows_subsystem = "windows"]
use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::path::{Path, PathBuf};

#[derive(Default)]
struct App {
    root: Option<PathBuf>,
    files: Vec<PathBuf>,
    current: Option<PathBuf>,
    text: String,
    dirty: bool,
    cache: CommonMarkCache,
    status: String,
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

impl App {
    fn open_folder(&mut self, dir: PathBuf) {
        let mut files = Vec::new();
        collect_md(&dir, &mut files);
        files.sort();
        self.files = files;
        self.root = Some(dir);
        self.current = None;
        self.text.clear();
        self.dirty = false;
    }

    fn save(&mut self) {
        if let Some(path) = &self.current {
            match std::fs::write(path, &self.text) {
                Ok(_) => {
                    self.dirty = false;
                    self.status = format!("Gespeichert: {}", path.display());
                }
                Err(e) => self.status = format!("Fehler beim Speichern: {e}"),
            }
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        if self.dirty {
            self.save(); // einfache Lösung: vor dem Wechsel speichern
        }
        match std::fs::read_to_string(&path) {
            Ok(t) => {
                self.text = t;
                self.current = Some(path);
                self.dirty = false;
            }
            Err(e) => self.status = format!("Fehler beim Lesen: {e}"),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Strg+S
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📂 Ordner öffnen").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.open_folder(dir);
                    }
                }
                if ui
                    .add_enabled(self.current.is_some(), egui::Button::new("💾 Speichern"))
                    .clicked()
                {
                    self.save();
                }
                if let Some(p) = &self.current {
                    ui.label(format!("{}{}", p.display(), if self.dirty { " *" } else { "" }));
                }
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
        });

        // Links: Dateiübersicht
        let mut clicked: Option<PathBuf> = None;
        egui::SidePanel::left("files")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Dateien");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for f in &self.files {
                        let label = match &self.root {
                            Some(r) => f.strip_prefix(r).unwrap_or(f).display().to_string(),
                            None => f.display().to_string(),
                        };
                        let selected = self.current.as_ref() == Some(f);
                        if ui.selectable_label(selected, label).clicked() {
                            clicked = Some(f.clone());
                        }
                    }
                });
            });
        if let Some(p) = clicked {
            self.open_file(p);
        }

        // Mitte: Editor links, Vorschau rechts, beide gleich breit
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                // Editor
                cols[0].heading("Editor");
                let h = cols[0].available_height();
                egui::ScrollArea::vertical()
                    .id_salt("editor_scroll")
                    .show(&mut cols[0], |ui| {
                        let resp = ui.add(
                            egui::TextEdit::multiline(&mut self.text)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .min_size(egui::vec2(0.0, h)),
                        );
                        if resp.changed() {
                            self.dirty = true;
                        }
                    });

                // Vorschau
                cols[1].heading("Vorschau");
                egui::ScrollArea::vertical()
                    .id_salt("preview_scroll")
                    .show(&mut cols[1], |ui| {
                        CommonMarkViewer::new().show(ui, &mut self.cache, &self.text);
                    });
            });
        });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "MD Editor",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}