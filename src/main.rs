#![windows_subsystem = "windows"]

mod app;
mod css;
mod doc;
mod editor;
mod highlight;
mod html_preview;
mod md2html;
mod single_instance;
mod system;
mod textops;
mod theme;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    let files: Vec<PathBuf> = std::env::args()
        .skip(1)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect();

    // Läuft schon eine Instanz? Dann Dateien dorthin weiterreichen und beenden,
    // damit sie in einem neuen Tab im bestehenden Fenster landen.
    let startup = single_instance::startup(&files);
    if matches!(startup, single_instance::Startup::Forwarded) {
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([760.0, 480.0])
            .with_title("MD Editor"),
        ..Default::default()
    };

    eframe::run_native(
        "MD Editor",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, startup)))),
    )
}
