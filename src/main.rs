#![windows_subsystem = "windows"]

use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::path::{Path, PathBuf};

const EDITOR_ID: &str = "markdown_editor";

// ---------------------------------------------------------------------------
// Zustand
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ViewMode {
    #[default]
    Split,
    Editor,
    Preview,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    fn preference(self) -> egui::ThemePreference {
        match self {
            Self::System => egui::ThemePreference::System,
            Self::Light => egui::ThemePreference::Light,
            Self::Dark => egui::ThemePreference::Dark,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Hell",
            Self::Dark => "Dunkel",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::System => "◐",
            Self::Light => "☀",
            Self::Dark => "☾",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::System => Self::Dark,
            Self::Dark => Self::Light,
            Self::Light => Self::System,
        }
    }
}

#[derive(Clone, Copy)]
enum FormatAction {
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    CodeBlock,
    Heading1,
    Heading2,
    Heading3,
    BulletList,
    NumberedList,
    Quote,
    Link,
    Image,
    Rule,
}

impl FormatAction {
    fn label(self) -> &'static str {
        match self {
            Self::Bold => "Fett",
            Self::Italic => "Kursiv",
            Self::Strikethrough => "Durchgestrichen",
            Self::InlineCode => "Inline-Code",
            Self::CodeBlock => "Codeblock",
            Self::Heading1 => "Überschrift 1",
            Self::Heading2 => "Überschrift 2",
            Self::Heading3 => "Überschrift 3",
            Self::BulletList => "Aufzählungsliste",
            Self::NumberedList => "Nummerierte Liste",
            Self::Quote => "Zitat",
            Self::Link => "Link",
            Self::Image => "Bild",
            Self::Rule => "Trennlinie",
        }
    }
}

#[derive(Default)]
struct App {
    root: Option<PathBuf>,
    files: Vec<PathBuf>,
    current: Option<PathBuf>,
    text: String,
    dirty: bool,
    cache: CommonMarkCache,
    status: String,

    view: ViewMode,
    show_files: bool,
    theme: ThemeMode,
    applied_theme: Option<ThemeMode>,

    recent: Vec<PathBuf>,
    about_open: bool,
    shortcuts_open: bool,
    window_title: String,
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
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
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let app = Self {
            status: "Bereit".to_owned(),
            show_files: true,
            ..Default::default()
        };
        app.apply_theme(&cc.egui_ctx);
        cc.egui_ctx.all_styles_mut(modern_style);
        app
    }

    // -- Design -------------------------------------------------------------

    fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(self.theme.preference());
    }

    // -- Dateien ------------------------------------------------------------

    fn has_document(&self) -> bool {
        self.current.is_some() || !self.text.is_empty()
    }

    fn open_folder(&mut self, ctx: &egui::Context, dir: PathBuf) {
        if self.root.as_ref() == Some(&dir) {
            return;
        }
        if self.dirty {
            self.save();
        }
        self.clear_current_document(ctx);

        let mut files = Vec::new();
        collect_md(&dir, &mut files);
        files.sort();
        self.files = files;
        self.root = Some(dir.clone());
        self.status = format!("Ordner geöffnet: {}", dir.display());
    }

    fn clear_current_document(&mut self, ctx: &egui::Context) {
        self.current = None;
        self.text.clear();
        self.dirty = false;
        self.reset_editor_state(ctx);
    }

    fn refresh_files(&mut self) {
        if let Some(root) = &self.root {
            let mut files = Vec::new();
            collect_md(root, &mut files);
            files.sort();
            self.files = files;
        }
    }

    fn push_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(10);
    }

    fn new_file(&mut self, ctx: &egui::Context) {
        if self.dirty {
            self.save();
        }
        self.current = None;
        self.text.clear();
        self.dirty = false;
        self.reset_editor_state(ctx);
        self.status = "Neue Datei – zum Speichern Strg+S drücken".to_owned();
    }

    fn open_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.current.as_ref() == Some(&path) {
            return;
        }
        if self.dirty {
            self.save();
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.text = text;
                self.dirty = false;
                self.current = Some(path.clone());
                self.reset_editor_state(ctx);
                self.push_recent(path.clone());
                if self.root.is_none() {
                    self.root = path.parent().map(Path::to_path_buf);
                    self.refresh_files();
                }
                self.status = format!("Geöffnet: {}", path.display());
            }
            Err(err) => self.status = format!("Fehler beim Lesen: {err}"),
        }
    }

    fn close_file(&mut self, ctx: &egui::Context) {
        if !self.has_document() {
            return;
        }
        if self.dirty {
            self.save();
        }
        let had_path = self.current.is_some();
        self.clear_current_document(ctx);
        self.status = if had_path {
            "Datei geschlossen".to_owned()
        } else {
            "Dokument geschlossen".to_owned()
        };
    }

    fn close_folder(&mut self, ctx: &egui::Context) {
        if self.root.is_none() {
            return;
        }
        if self.dirty {
            self.save();
        }
        self.clear_current_document(ctx);
        self.files.clear();
        self.root = None;
        self.status = "Ordner geschlossen".to_owned();
    }

    fn pick_file(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().add_filter("Markdown", &["md", "markdown"]);
        if let Some(root) = &self.root {
            dialog = dialog.set_directory(root);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_file(ctx, path);
        }
    }

    fn pick_folder(&mut self, ctx: &egui::Context) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.open_folder(ctx, dir);
        }
    }

    fn save(&mut self) {
        let Some(path) = self.current.clone() else {
            self.save_as();
            return;
        };
        match std::fs::write(&path, &self.text) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("Gespeichert: {}", path.display());
            }
            Err(err) => self.status = format!("Fehler beim Speichern: {err}"),
        }
    }

    fn save_as(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .set_file_name(
                self.current
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Unbenannt.md".to_owned()),
            );
        if let Some(dir) = self
            .current
            .as_ref()
            .and_then(|p| p.parent())
            .or(self.root.as_deref())
        {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("md");
        }
        self.current = Some(path.clone());
        match std::fs::write(&path, &self.text) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("Gespeichert: {}", path.display());
            }
            Err(err) => self.status = format!("Fehler beim Speichern: {err}"),
        }
        self.push_recent(path.clone());
        if self.root.is_none() {
            self.root = path.parent().map(Path::to_path_buf);
        }
        self.refresh_files();
    }

    // -- Editor-Status ------------------------------------------------------

    fn reset_editor_state(&self, ctx: &egui::Context) {
        let id = egui::Id::new(EDITOR_ID);
        if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
            state.clear_undoer();
            state
                .cursor
                .set_char_range(Some(CCursorRange::one(CCursor::new(0))));
            egui::TextEdit::store_state(ctx, id, state);
        }
    }

    fn focus_editor(ctx: &egui::Context) {
        ctx.memory_mut(|mem| mem.request_focus(egui::Id::new(EDITOR_ID)));
    }

    fn select_all(&self, ctx: &egui::Context) {
        let id = egui::Id::new(EDITOR_ID);
        let Some(mut state) = egui::TextEdit::load_state(ctx, id) else {
            return;
        };
        let len = self.text.chars().count();
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
        egui::TextEdit::store_state(ctx, id, state);
    }

    fn undo(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new(EDITOR_ID);
        let Some(mut state) = egui::TextEdit::load_state(ctx, id) else {
            return;
        };
        let range = state
            .cursor
            .char_range()
            .unwrap_or_else(|| CCursorRange::one(CCursor::new(self.text.chars().count())));
        let current = (range, self.text.clone());
        let mut undoer = state.undoer();
        let Some(previous) = undoer.undo(&current).cloned() else {
            return;
        };
        self.text = previous.1;
        state.cursor.set_char_range(Some(previous.0));
        state.set_undoer(undoer);
        egui::TextEdit::store_state(ctx, id, state);
        self.dirty = true;
        self.status = "Rückgängig".to_owned();
    }

    fn redo(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new(EDITOR_ID);
        let Some(mut state) = egui::TextEdit::load_state(ctx, id) else {
            return;
        };
        let range = state
            .cursor
            .char_range()
            .unwrap_or_else(|| CCursorRange::one(CCursor::new(self.text.chars().count())));
        let current = (range, self.text.clone());
        let mut undoer = state.undoer();
        let Some(next) = undoer.redo(&current).cloned() else {
            return;
        };
        self.text = next.1;
        state.cursor.set_char_range(Some(next.0));
        state.set_undoer(undoer);
        egui::TextEdit::store_state(ctx, id, state);
        self.dirty = true;
        self.status = "Wiederholt".to_owned();
    }

    fn pick_image(&mut self) -> Option<(String, String)> {
        let Some(doc_dir) = self
            .current
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
        else {
            self.status = "Bitte die Datei zuerst speichern (für den relativen Bildpfad)".into();
            return None;
        };
        let file = rfd::FileDialog::new()
            .add_filter("Bilder", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff"])
            .set_directory(&doc_dir)
            .pick_file()?;

        let alt = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rel = relative_path(&doc_dir, &file).unwrap_or_else(|| file.clone());
        let mut dest = rel.to_string_lossy().replace('\\', "/");
        if dest.contains([' ', '(', ')']) {
            dest = format!("<{dest}>");
        }
        Some((alt, dest))
    }

    fn apply_format(&mut self, ctx: &egui::Context, action: FormatAction) {
        let image = if matches!(action, FormatAction::Image) {
            match self.pick_image() {
                Some(i) => Some(i),
                None => return,
            }
        } else {
            None
        };

        let id = egui::Id::new(EDITOR_ID);
        let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        let range = state
            .cursor
            .char_range()
            .unwrap_or_else(|| CCursorRange::one(CCursor::new(self.text.chars().count())));
        let start = range.primary.index.min(range.secondary.index);
        let end = range.primary.index.max(range.secondary.index);

        // Undo-Punkt vor der Änderung anlegen.
        let mut undoer = state.undoer();
        undoer.add_undo(&(range, self.text.clone()));

        let (new_start, new_end) = match action {
            FormatAction::Bold => wrap_selection(&mut self.text, start, end, "**", "**"),
            FormatAction::Italic => wrap_selection(&mut self.text, start, end, "*", "*"),
            FormatAction::Strikethrough => wrap_selection(&mut self.text, start, end, "~~", "~~"),
            FormatAction::InlineCode => wrap_selection(&mut self.text, start, end, "`", "`"),
            FormatAction::CodeBlock => wrap_selection(&mut self.text, start, end, "```\n", "\n```"),
            FormatAction::Heading1 => prefix_lines(&mut self.text, start, end, "# "),
            FormatAction::Heading2 => prefix_lines(&mut self.text, start, end, "## "),
            FormatAction::Heading3 => prefix_lines(&mut self.text, start, end, "### "),
            FormatAction::BulletList => prefix_lines(&mut self.text, start, end, "- "),
            FormatAction::NumberedList => prefix_lines(&mut self.text, start, end, "1. "),
            FormatAction::Quote => prefix_lines(&mut self.text, start, end, "> "),
            FormatAction::Link => insert_link(&mut self.text, start, end),
            FormatAction::Image => {
                let (alt, dest) = image.clone().unwrap_or_default();
                insert_image(&mut self.text, start, end, &alt, &dest)
            }
            FormatAction::Rule => {
                let pos = replace_range(&mut self.text, start, end, "\n\n---\n\n");
                (pos + 7, pos + 7)
            }
        };

        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(new_start),
            CCursor::new(new_end),
        )));
        state.set_undoer(undoer);
        egui::TextEdit::store_state(ctx, id, state);
        self.dirty = true;
        self.status = format!("{} eingefügt", action.label());
    }

    // -- Tastenkürzel -------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};

        // Zuerst die Variante mit Umschalt prüfen, da sie sonst von der
        // einfachen Kombination geschluckt würde.
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)) {
            self.save_as();
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            self.pick_file(ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::N)) {
            self.new_file(ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::W)) {
            self.close_file(ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::B)) {
            self.apply_format(ctx, FormatAction::Bold);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::I)) {
            self.apply_format(ctx, FormatAction::Italic);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::K)) {
            self.apply_format(ctx, FormatAction::Link);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num1)) {
            self.apply_format(ctx, FormatAction::Heading1);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num2)) {
            self.apply_format(ctx, FormatAction::Heading2);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num3)) {
            self.apply_format(ctx, FormatAction::Heading3);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F11)) {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.current {
            Some(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                format!("{}{} – MD Editor", name, if self.dirty { " •" } else { "" })
            }
            None => "Unbenannt – MD Editor".to_owned(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    // -- UI -----------------------------------------------------------------

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                file_menu(ui, self);
                edit_menu(ui, self);
                format_menu(ui, self);
                view_menu(ui, self);
                help_menu(ui, self);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(self.theme.icon()).frame(false))
                        .on_hover_text(format!(
                            "Design: {} – klicken zum Wechseln",
                            self.theme.label()
                        ))
                        .clicked()
                    {
                        self.theme = self.theme.next();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(self.has_document(), egui::Button::new("💾").frame(false))
                        .on_hover_text("Speichern (Strg+S)")
                        .clicked()
                    {
                        self.save();
                    }
                    if ui
                        .add(egui::Button::new("📂").frame(false))
                        .on_hover_text("Datei öffnen (Strg+O)")
                        .clicked()
                    {
                        self.pick_file(ctx);
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.small(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let chars = self.text.chars().count();
                    let words = self.text.split_whitespace().count();
                    let lines = self.text.lines().count().max(1);
                    ui.small(format!("{lines} Zeilen · {words} Wörter · {chars} Zeichen"));
                });
            });
        });
    }

    fn file_panel(&mut self, ctx: &egui::Context) {
        if !self.show_files {
            return;
        }

        let mut clicked: Option<PathBuf> = None;
        egui::SidePanel::left("file_list")
            .resizable(true)
            .default_width(240.0)
            .width_range(180.0..=460.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.strong("Dateien");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("⟳")
                            .on_hover_text("Dateiliste aktualisieren")
                            .clicked()
                        {
                            self.refresh_files();
                        }
                        if ui
                            .small_button("📂")
                            .on_hover_text("Ordner öffnen")
                            .clicked()
                        {
                            self.pick_folder(ctx);
                        }
                        if ui
                            .add_enabled(self.root.is_some(), egui::Button::new("✕").frame(false).small())
                            .on_hover_text("Ordner schließen")
                            .clicked()
                        {
                            self.close_folder(ctx);
                        }
                    });
                });

                match &self.root {
                    Some(root) => {
                        let name = root
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| root.display().to_string());
                        ui.weak(name).on_hover_text(root.display().to_string());
                    }
                    None => {
                        ui.weak("Kein Ordner geöffnet");
                    }
                }

                ui.separator();

                if self.files.is_empty() {
                    ui.add_space(6.0);
                    ui.weak(if self.root.is_some() {
                        "Keine Markdown-Dateien gefunden."
                    } else {
                        "Öffne einen Ordner, um deine Dateien zu sehen."
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for file in &self.files {
                                let label = self
                                    .root
                                    .as_ref()
                                    .and_then(|root| file.strip_prefix(root).ok())
                                    .unwrap_or(file)
                                    .display()
                                    .to_string();
                                let selected = self.current.as_ref() == Some(file);
                                if ui.selectable_label(selected, label).clicked() {
                                    clicked = Some(file.clone());
                                }
                            }
                        });
                }
            });

        if let Some(path) = clicked {
            self.open_file(ctx, path);
        }
    }

    fn central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.editor_toolbar(ui);
            ui.separator();

            match self.view {
                ViewMode::Split => {
                    ui.columns(2, |columns| {
                        editor_pane(&mut columns[0], self);
                        preview_pane(&mut columns[1], self);
                    });
                }
                ViewMode::Editor => editor_pane(ui, self),
                ViewMode::Preview => preview_pane(ui, self),
            }
        });
    }

    fn editor_toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            let name = self
                .current
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unbenannt".to_owned());

            let title = ui.strong(name);
            if let Some(path) = &self.current {
                title.on_hover_text(path.display().to_string());
            }
            if self.dirty {
                ui.colored_label(ui.visuals().warn_fg_color, "•")
                    .on_hover_text("Ungespeicherte Änderungen");
            }

            if ui
                .add_enabled(
                    self.has_document(),
                    egui::Button::new("✕").frame(false).small(),
                )
                .on_hover_text("Datei schließen (Strg+W)")
                .clicked()
            {
                self.close_file(&ctx);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut self.view, ViewMode::Preview, "Vorschau");
                ui.selectable_value(&mut self.view, ViewMode::Editor, "Editor");
                ui.selectable_value(&mut self.view, ViewMode::Split, "Geteilt");
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.about_open {
            let modal = egui::Modal::new(egui::Id::new("about_modal")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("MD Editor");
                ui.add_space(4.0);
                ui.label("Ein schlanker Markdown-Editor mit Live-Vorschau.");
                ui.add_space(10.0);
                egui::Grid::new("about_grid")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.weak("Version");
                        ui.label(env!("CARGO_PKG_VERSION"));
                        ui.end_row();
                        ui.weak("Framework");
                        ui.label("egui / eframe");
                        ui.end_row();
                        ui.weak("Markdown");
                        ui.label("egui_commonmark");
                        ui.end_row();
                    });
                ui.add_space(12.0);
                if ui.button("Schließen").clicked() {
                    ui.close();
                }
            });
            if modal.should_close() {
                self.about_open = false;
            }
        }

        if self.shortcuts_open {
            let modal = egui::Modal::new(egui::Id::new("shortcuts_modal")).show(ctx, |ui| {
                ui.set_width(430.0);
                ui.heading("Tastenkürzel");
                ui.add_space(8.0);
                egui::Grid::new("shortcut_grid")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        let shortcuts = [
                            ("Strg+N", "Neue Datei"),
                            ("Strg+O", "Datei öffnen"),
                            ("Strg+S", "Speichern"),
                            ("Strg+Umschalt+S", "Speichern unter"),
                            ("Strg+W", "Datei schließen"),
                            ("Strg+B", "Fett"),
                            ("Strg+I", "Kursiv"),
                            ("Strg+K", "Link einfügen"),
                            ("Strg+1/2/3", "Überschrift 1/2/3"),
                            ("Strg+Z", "Rückgängig"),
                            ("Strg+Umschalt+Z", "Wiederholen"),
                            ("F11", "Vollbild umschalten"),
                        ];
                        for (keys, action) in shortcuts {
                            ui.monospace(keys);
                            ui.label(action);
                            ui.end_row();
                        }
                    });
                ui.add_space(12.0);
                if ui.button("Schließen").clicked() {
                    ui.close();
                }
            });
            if modal.should_close() {
                self.shortcuts_open = false;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.applied_theme != Some(self.theme) {
            self.apply_theme(ctx);
            self.applied_theme = Some(self.theme);
        }

        // Beim Schließen ungespeicherte Änderungen sichern, sofern die Datei
        // bereits einen Pfad hat.
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && self.current.is_some() {
            self.save();
        }

        self.handle_shortcuts(ctx);

        self.menu_bar(ctx);
        self.status_bar(ctx);
        self.file_panel(ctx);
        self.central_panel(ctx);
        self.dialogs(ctx);

        self.update_title(ctx);
    }
}

// ---------------------------------------------------------------------------
// Menüs
// ---------------------------------------------------------------------------

fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: Option<&str>) -> bool {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(shortcut);
    }
    ui.add(button).clicked()
}

fn file_menu(ui: &mut egui::Ui, app: &mut App) {
    ui.menu_button("Datei", |ui| {
        let ctx = ui.ctx().clone();

        if menu_item(ui, "📄  Neu", Some("Strg+N")) {
            app.new_file(&ctx);
            ui.close();
        }
        if menu_item(ui, "📂  Datei öffnen …", Some("Strg+O")) {
            app.pick_file(&ctx);
            ui.close();
        }
        if menu_item(ui, "🗂  Ordner öffnen …", None) {
            app.pick_folder(&ctx);
            ui.close();
        }

        ui.separator();

        if ui
            .add_enabled(
                app.has_document(),
                egui::Button::new("💾  Speichern").shortcut_text("Strg+S"),
            )
            .clicked()
        {
            app.save();
            ui.close();
        }
        if menu_item(ui, "💾  Speichern unter …", Some("Strg+Umschalt+S")) {
            app.save_as();
            ui.close();
        }
        if ui
            .add_enabled(
                app.has_document(),
                egui::Button::new("✕  Datei schließen").shortcut_text("Strg+W"),
            )
            .clicked()
        {
            app.close_file(&ctx);
            ui.close();
        }
        if ui
            .add_enabled(app.root.is_some(), egui::Button::new("🗂  Ordner schließen"))
            .clicked()
        {
            app.close_folder(&ctx);
            ui.close();
        }

        ui.separator();

        ui.add_enabled_ui(!app.recent.is_empty(), |ui| {
            ui.menu_button("🕘  Zuletzt geöffnet", |ui| {
                let recent = app.recent.clone();
                for path in recent {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    if ui
                        .add(egui::Button::new(name))
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        app.open_file(ui.ctx(), path);
                        ui.close();
                    }
                }
                ui.separator();
                if menu_item(ui, "Liste leeren", None) {
                    app.recent.clear();
                    ui.close();
                }
            });
        });

        ui.separator();

        if menu_item(ui, "Beenden", None) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

fn edit_menu(ui: &mut egui::Ui, app: &mut App) {
    ui.menu_button("Bearbeiten", |ui| {
        let ctx = ui.ctx().clone();
        let has_document = app.has_document();

        if ui
            .add_enabled(
                has_document,
                egui::Button::new("↩  Rückgängig").shortcut_text("Strg+Z"),
            )
            .clicked()
        {
            app.undo(&ctx);
            ui.close();
        }
        if ui
            .add_enabled(
                has_document,
                egui::Button::new("↪  Wiederholen").shortcut_text("Strg+Umschalt+Z"),
            )
            .clicked()
        {
            app.redo(&ctx);
            ui.close();
        }

        ui.separator();

        if ui
            .add_enabled(
                has_document,
                egui::Button::new("✂  Ausschneiden").shortcut_text("Strg+X"),
            )
            .clicked()
        {
            App::focus_editor(&ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut);
            ui.close();
        }
        if ui
            .add_enabled(
                has_document,
                egui::Button::new("⧉  Kopieren").shortcut_text("Strg+C"),
            )
            .clicked()
        {
            App::focus_editor(&ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy);
            ui.close();
        }
        if ui
            .add_enabled(
                has_document,
                egui::Button::new("📋  Einfügen").shortcut_text("Strg+V"),
            )
            .clicked()
        {
            App::focus_editor(&ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }

        ui.separator();

        if ui
            .add_enabled(
                has_document,
                egui::Button::new("▭  Alles auswählen").shortcut_text("Strg+A"),
            )
            .clicked()
        {
            app.select_all(&ctx);
            App::focus_editor(&ctx);
            ui.close();
        }
    });
}

fn format_menu(ui: &mut egui::Ui, app: &mut App) {
    ui.menu_button("Format", |ui| {
        let ctx = ui.ctx().clone();

        format_item(ui, app, &ctx, "Fett", Some("Strg+B"), FormatAction::Bold);
        format_item(
            ui,
            app,
            &ctx,
            "Kursiv",
            Some("Strg+I"),
            FormatAction::Italic,
        );
        format_item(
            ui,
            app,
            &ctx,
            "Durchgestrichen",
            None,
            FormatAction::Strikethrough,
        );
        format_item(ui, app, &ctx, "Inline-Code", None, FormatAction::InlineCode);
        format_item(ui, app, &ctx, "Codeblock", None, FormatAction::CodeBlock);

        ui.separator();

        format_item(
            ui,
            app,
            &ctx,
            "Überschrift 1",
            Some("Strg+1"),
            FormatAction::Heading1,
        );
        format_item(
            ui,
            app,
            &ctx,
            "Überschrift 2",
            Some("Strg+2"),
            FormatAction::Heading2,
        );
        format_item(
            ui,
            app,
            &ctx,
            "Überschrift 3",
            Some("Strg+3"),
            FormatAction::Heading3,
        );

        ui.separator();

        format_item(
            ui,
            app,
            &ctx,
            "Aufzählungsliste",
            None,
            FormatAction::BulletList,
        );
        format_item(
            ui,
            app,
            &ctx,
            "Nummerierte Liste",
            None,
            FormatAction::NumberedList,
        );
        format_item(ui, app, &ctx, "Zitat", None, FormatAction::Quote);

        ui.separator();

        format_item(ui, app, &ctx, "Link", Some("Strg+K"), FormatAction::Link);
        format_item(ui, app, &ctx, "Bild einfügen …", None, FormatAction::Image);
        format_item(ui, app, &ctx, "Trennlinie", None, FormatAction::Rule);
    });
}

fn format_item(
    ui: &mut egui::Ui,
    app: &mut App,
    ctx: &egui::Context,
    label: &str,
    shortcut: Option<&str>,
    action: FormatAction,
) {
    if menu_item(ui, label, shortcut) {
        app.apply_format(ctx, action);
        ui.close();
    }
}

fn view_menu(ui: &mut egui::Ui, app: &mut App) {
    ui.menu_button("Ansicht", |ui| {
        if ui.checkbox(&mut app.show_files, "Dateiliste").clicked() {
            ui.close();
        }

        ui.separator();

        if ui
            .selectable_value(&mut app.view, ViewMode::Split, "Geteilte Ansicht")
            .clicked()
        {
            ui.close();
        }
        if ui
            .selectable_value(&mut app.view, ViewMode::Editor, "Nur Editor")
            .clicked()
        {
            ui.close();
        }
        if ui
            .selectable_value(&mut app.view, ViewMode::Preview, "Nur Vorschau")
            .clicked()
        {
            ui.close();
        }

        ui.separator();

        ui.menu_button("🎨  Design", |ui| {
            if ui
                .selectable_value(&mut app.theme, ThemeMode::System, "◐  System")
                .clicked()
            {
                ui.close();
            }
            if ui
                .selectable_value(&mut app.theme, ThemeMode::Light, "☀  Hell")
                .clicked()
            {
                ui.close();
            }
            if ui
                .selectable_value(&mut app.theme, ThemeMode::Dark, "☾  Dunkel")
                .clicked()
            {
                ui.close();
            }
        });

        ui.separator();

        let fullscreen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
        let label = if fullscreen {
            "Vollbild verlassen"
        } else {
            "Vollbild"
        };
        if menu_item(ui, label, Some("F11")) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            ui.close();
        }
    });
}

fn help_menu(ui: &mut egui::Ui, app: &mut App) {
    ui.menu_button("Hilfe", |ui| {
        if menu_item(ui, "⌨  Tastenkürzel", None) {
            app.shortcuts_open = true;
            ui.close();
        }
        if menu_item(ui, "ℹ  Über MD Editor", None) {
            app.about_open = true;
            ui.close();
        }
    });
}

// ---------------------------------------------------------------------------
// Editor & Vorschau
// ---------------------------------------------------------------------------

fn editor_pane(ui: &mut egui::Ui, app: &mut App) {
    ui.horizontal(|ui| {
        ui.strong("✏  Editor");
        if app.dirty {
            ui.colored_label(ui.visuals().warn_fg_color, "ungespeichert");
        }
    });
    ui.add_space(4.0);

    let height = ui.available_height();
    egui::ScrollArea::vertical()
        .id_salt("editor_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let output = egui::TextEdit::multiline(&mut app.text)
                .id(egui::Id::new(EDITOR_ID))
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(24)
                .min_size(egui::vec2(0.0, height))
                .hint_text("Markdown hier schreiben …")
                .show(ui);

            if output.response.changed() {
                app.dirty = true;
            }

            output.response.context_menu(|ui| {
                let ctx = ui.ctx().clone();
                format_item(ui, app, &ctx, "Fett", None, FormatAction::Bold);
                format_item(ui, app, &ctx, "Kursiv", None, FormatAction::Italic);
                format_item(ui, app, &ctx, "Inline-Code", None, FormatAction::InlineCode);
                format_item(ui, app, &ctx, "Codeblock", None, FormatAction::CodeBlock);
                ui.separator();
                format_item(ui, app, &ctx, "Link", None, FormatAction::Link);
                format_item(
                    ui,
                    app,
                    &ctx,
                    "Aufzählungsliste",
                    None,
                    FormatAction::BulletList,
                );
                format_item(
                    ui,
                    app,
                    &ctx,
                    "Nummerierte Liste",
                    None,
                    FormatAction::NumberedList,
                );
                format_item(ui, app, &ctx, "Zitat", None, FormatAction::Quote);
            });
        });
}

fn preview_pane(ui: &mut egui::Ui, app: &mut App) {
    ui.horizontal(|ui| {
        ui.strong("👁  Vorschau");
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .id_salt("preview_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if app.text.trim().is_empty() {
                ui.weak("Die Vorschau erscheint hier.");
            } else {
                // Relative image paths in Markdown are resolved from the directory
                // containing the currently opened Markdown file, not the process cwd.
                let image_base = app
                    .current
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(|path| url::Url::from_directory_path(path).ok());

                let viewer = match image_base {
                    Some(base) => CommonMarkViewer::new()
                        .default_implicit_uri_scheme(base.to_string()),
                    None => CommonMarkViewer::new(),
                };
                viewer.show(ui, &mut app.cache, &app.text);
            }
        });
}

// ---------------------------------------------------------------------------
// Text-Helfer
// ---------------------------------------------------------------------------

fn modern_style(style: &mut egui::Style) {
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(8);
    for widget in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(6);
    }
}

fn char_to_byte(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

/// Ersetzt den Bereich `[start, end)` (Zeichenindizes) durch `replacement`
/// und liefert den Zeichenindex, an dem die Ersetzung beginnt.
fn replace_range(text: &mut String, start: usize, end: usize, replacement: &str) -> usize {
    let (start, end) = (start.min(end), start.max(end));
    let start_byte = char_to_byte(text, start);
    let end_byte = char_to_byte(text, end);

    let mut result = String::with_capacity(text.len() + replacement.len());
    result.push_str(&text[..start_byte]);
    let replaced_at = result.len();
    result.push_str(replacement);
    result.push_str(&text[end_byte..]);
    *text = result;

    text[..replaced_at].chars().count()
}

/// Umschließt die Auswahl mit `before`/`after` und liefert die neue Auswahl
/// (bzw. Cursorposition, wenn nichts ausgewählt war).
fn wrap_selection(
    text: &mut String,
    start: usize,
    end: usize,
    before: &str,
    after: &str,
) -> (usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    let start_byte = char_to_byte(text, start);
    let end_byte = char_to_byte(text, end);
    let selected = text[start_byte..end_byte].to_string();

    let mut result = String::with_capacity(text.len() + before.len() + after.len());
    result.push_str(&text[..start_byte]);
    result.push_str(before);
    let inner_start_byte = result.len();
    result.push_str(&selected);
    result.push_str(after);
    result.push_str(&text[end_byte..]);
    *text = result;

    let inner_start = text[..inner_start_byte].chars().count();
    let inner_end = inner_start + selected.chars().count();
    (inner_start, inner_end)
}

/// Stellt jeder Zeile der Auswahl `prefix` voran.
fn prefix_lines(text: &mut String, start: usize, end: usize, prefix: &str) -> (usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    let start_byte = char_to_byte(text, start);
    let end_byte = char_to_byte(text, end);

    let line_start_byte = text[..start_byte].rfind('\n').map_or(0, |i| i + 1);
    let line_end_byte = if end_byte > line_start_byte && text[..end_byte].ends_with('\n') {
        end_byte
    } else {
        text[end_byte..]
            .find('\n')
            .map_or(text.len(), |i| end_byte + i)
    };

    let block = text[line_start_byte..line_end_byte].to_string();
    let mut new_block = String::with_capacity(block.len() + prefix.len() * 8);
    for (index, line) in block.split('\n').enumerate() {
        if index > 0 {
            new_block.push('\n');
        }
        new_block.push_str(prefix);
        new_block.push_str(line);
    }

    let mut result = String::with_capacity(text.len() + new_block.len());
    result.push_str(&text[..line_start_byte]);
    let block_start_byte = result.len();
    result.push_str(&new_block);
    result.push_str(&text[line_end_byte..]);
    *text = result;

    let block_start = text[..block_start_byte].chars().count();
    let block_end = block_start + new_block.chars().count();
    (block_start, block_end)
}

/// Fügt einen Markdown-Link ein und wählt den sinnvollen Teil aus.
fn insert_link(text: &mut String, start: usize, end: usize) -> (usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    let start_byte = char_to_byte(text, start);
    let end_byte = char_to_byte(text, end);
    let selected = text[start_byte..end_byte].to_string();

    if selected.is_empty() {
        let pos = replace_range(text, start, end, "[Linktext](url)");
        let text_start = pos + 1;
        (text_start, text_start + "Linktext".chars().count())
    } else {
        let replacement = format!("[{selected}](url)");
        let pos = replace_range(text, start, end, &replacement);
        let url_start = pos + 1 + selected.chars().count() + 2;
        (url_start, url_start + 3)
    }
}

/// Relativer Pfad von `from_dir` nach `to`; None bei unterschiedlichem Laufwerk.
fn relative_path(from_dir: &Path, to: &Path) -> Option<PathBuf> {
    let from: Vec<_> = from_dir.components().collect();
    let target: Vec<_> = to.components().collect();
    let common = from.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut rel = PathBuf::new();
    for _ in common..from.len() {
        rel.push("..");
    }
    for c in &target[common..] {
        rel.push(c.as_os_str());
    }
    Some(rel)
}

/// Fügt `![alt](pfad)` ein und markiert den Alternativtext zum direkten Überschreiben.
fn insert_image(
    text: &mut String,
    start: usize,
    end: usize,
    alt: &str,
    dest: &str,
) -> (usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    let (sb, eb) = (char_to_byte(text, start), char_to_byte(text, end));
    let selected = text[sb..eb].replace('\n', " ");
    let alt = if selected.trim().is_empty() {
        alt.to_string()
    } else {
        selected
    };

    let pos = replace_range(text, start, end, &format!("![{alt}]({dest})"));
    let alt_start = pos + 2;
    (alt_start, alt_start + alt.chars().count())
}

// ---------------------------------------------------------------------------

fn main() -> eframe::Result {
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
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}