use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{
    self,
    Align2, Color32, FontId, Key, Modifiers, RichText, TextEdit,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::doc::{
    collect_files, display_path, is_supported, DocKind, Document, FileStamp, ScrollMaster,
    HTML_EXTENSIONS, MARKDOWN_EXTENSIONS,
};
use crate::editor::{self, EditorOptions};
use crate::html_preview::ImageCache;
use crate::md2html::{self, CssChoice, ExportOptions};
use crate::single_instance::Startup;
use crate::textops::FormatAction;
use crate::theme::{self, ThemeMode};
use crate::system;

const APP_NAME: &str = "MD Editor";

/// Abstand, in dem geöffnete Dateien auf externe Änderungen geprüft werden.
const FILE_WATCH_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Split,
    Editor,
    Preview,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CloseTarget {
    Document(u64),
    Folder,
    Window,
}

struct ExportState {
    doc_id: u64,
    options: ExportOptions,
}

pub struct App {
    pub docs: Vec<Document>,
    pub active: Option<u64>,
    next_id: u64,
    next_untitled: u32,

    root: Option<PathBuf>,
    files: Vec<PathBuf>,
    recent: Vec<PathBuf>,

    cache: CommonMarkCache,
    status: String,

    view: ViewMode,
    show_files: bool,
    theme: ThemeMode,
    applied_theme: Option<ThemeMode>,

    sync_scroll: bool,
    auto_indent: bool,
    line_numbers: bool,
    syntax_highlighting: bool,

    about_open: bool,
    shortcuts_open: bool,
    export: Option<ExportState>,
    pending_close: Option<CloseTarget>,
    window_close_allowed: bool,

    window_title: String,
    images: ImageCache,

    /// Dateien, die eine weitere Instanz per „Öffnen mit“ weiterreicht.
    incoming: Option<Receiver<PathBuf>>,

    /// Zeitpunkt der letzten Prüfung auf externe Dateiänderungen.
    last_watch: Option<Instant>,
    /// Dokumente, deren Datei extern geändert wurde und die noch entschieden werden müssen.
    reload_requests: Vec<u64>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        let incoming = match startup {
            Startup::Primary(primary) => Some(primary.attach(cc.egui_ctx.clone())),
            _ => None,
        };
        let mut app = Self {
            docs: Vec::new(),
            active: None,
            next_id: 1,
            next_untitled: 1,
            root: None,
            files: Vec::new(),
            recent: Vec::new(),
            cache: CommonMarkCache::default(),
            status: "Bereit".to_owned(),
            view: ViewMode::Split,
            show_files: true,
            theme: ThemeMode::System,
            applied_theme: None,
            sync_scroll: true,
            auto_indent: true,
            line_numbers: true,
            syntax_highlighting: true,
            about_open: false,
            shortcuts_open: false,
            export: None,
            pending_close: None,
            window_close_allowed: false,
            window_title: String::new(),
            images: ImageCache::default(),
            incoming,
            last_watch: None,
            reload_requests: Vec::new(),
        };
        app.apply_theme(&cc.egui_ctx);
        cc.egui_ctx.all_styles_mut(theme::modern_style);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        app.open_command_line_files();
        app
    }

    // -- Dokumente ----------------------------------------------------------

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn next_untitled_name(&mut self) -> String {
        let number = self.next_untitled;
        self.next_untitled += 1;
        if number == 1 {
            "Unbenannt".to_owned()
        } else {
            format!("Unbenannt {number}")
        }
    }

    pub fn active_index(&self) -> Option<usize> {
        let id = self.active?;
        self.docs.iter().position(|doc| doc.id == id)
    }

    pub fn active_doc(&self) -> Option<&Document> {
        self.active_index().map(|index| &self.docs[index])
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut Document> {
        self.active_index().map(|index| &mut self.docs[index])
    }

    pub fn has_document(&self) -> bool {
        self.active.is_some()
    }

    fn new_document(&mut self, ctx: &egui::Context, kind: DocKind) {
        let id = self.alloc_id();
        let name = self.next_untitled_name();
        let doc = Document::untitled(id, kind, name);
        self.docs.push(doc);
        self.active = Some(id);
        editor::focus(ctx, self.docs.last().unwrap());
        self.status = match kind {
            DocKind::Markdown => "Neues Markdown-Dokument – zum Speichern Strg+S drücken".to_owned(),
            DocKind::Html => "Neues HTML-Dokument – zum Speichern Strg+S drücken".to_owned(),
        };
    }

    fn open_command_line_files(&mut self) {
        let args: Vec<PathBuf> = std::env::args()
            .skip(1)
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .collect();
        for path in args {
            self.open_path_silent(path);
        }
    }

    fn open_path_silent(&mut self, path: PathBuf) {
        if let Some(existing) = self
            .docs
            .iter()
            .find(|doc| doc.path.as_deref() == Some(path.as_path()))
        {
            self.active = Some(existing.id);
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.status = format!("Fehler beim Lesen: {}", path.display());
            return;
        };
        let id = self.alloc_id();
        let doc = Document::from_path(id, path.clone(), text);
        self.docs.push(doc);
        self.active = Some(id);
        self.push_recent(path.clone());
        if self.root.is_none() {
            self.root = path.parent().map(Path::to_path_buf);
            self.refresh_files();
        }
        self.status = format!("Geöffnet: {}", path.display());
    }

    /// Öffnet Dateien, die eine andere Instanz an dieses Fenster übergeben hat.
    fn handle_incoming_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = match &self.incoming {
            Some(receiver) => {
                let mut paths = Vec::new();
                while let Ok(path) = receiver.try_recv() {
                    paths.push(path);
                }
                paths
            }
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        for path in paths {
            self.open_path_silent(path);
        }
        if let Some(doc) = self.active_doc() {
            editor::focus(ctx, doc);
        }
        self.refresh_files();
        // Fenster in den Vordergrund holen, damit der neue Tab sichtbar ist.
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.update_title(ctx);
    }

    pub fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        if let Some(existing) = self
            .docs
            .iter()
            .find(|doc| doc.path.as_deref() == Some(path.as_path()))
        {
            self.active = Some(existing.id);
            editor::focus(ctx, existing);
            self.status = format!("Geöffnet: {}", path.display());
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let id = if let Some(index) = self.active_index() {
                    let reusable = {
                        let doc = &self.docs[index];
                        doc.path.is_none() && doc.text.trim().is_empty() && !doc.dirty
                    };
                    if reusable {
                        let id = self.docs[index].id;
                        self.docs[index] = Document::from_path(id, path.clone(), text);
                        id
                    } else {
                        let id = self.alloc_id();
                        self.docs
                            .push(Document::from_path(id, path.clone(), text));
                        id
                    }
                } else {
                    let id = self.alloc_id();
                    self.docs
                        .push(Document::from_path(id, path.clone(), text));
                    id
                };
                self.active = Some(id);
                editor::focus(ctx, self.docs.iter().find(|doc| doc.id == id).unwrap());
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

    fn pick_file(&mut self, ctx: &egui::Context) {
        let mut extensions: Vec<&str> = MARKDOWN_EXTENSIONS.to_vec();
        extensions.extend_from_slice(HTML_EXTENSIONS);
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Unterstützte Dateien", &extensions)
            .add_filter("Markdown", MARKDOWN_EXTENSIONS)
            .add_filter("HTML", HTML_EXTENSIONS);
        if let Some(root) = &self.root {
            dialog = dialog.set_directory(root);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_path(ctx, path);
        }
    }

    fn pick_folder(&mut self, ctx: &egui::Context) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.open_folder(ctx, dir);
        }
    }

    fn open_folder(&mut self, ctx: &egui::Context, dir: PathBuf) {
        if self.root.as_ref() == Some(&dir) {
            return;
        }
        let mut files = Vec::new();
        collect_files(&dir, &mut files);
        files.sort();
        self.files = files;
        self.root = Some(dir.clone());
        self.status = format!("Ordner geöffnet: {}", dir.display());

        // Wenn noch kein Dokument offen ist, die erste Datei anzeigen.
        if self.docs.is_empty() {
            if let Some(first) = self.files.first().cloned() {
                self.open_path(ctx, first);
            }
        }
    }

    fn refresh_files(&mut self) {
        if let Some(root) = &self.root {
            let mut files = Vec::new();
            collect_files(root, &mut files);
            files.sort();
            self.files = files;
        }
    }

    fn push_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(10);
    }

    // -- Speichern ----------------------------------------------------------

    fn save_document(&mut self, id: u64) -> bool {
        let Some(index) = self.docs.iter().position(|doc| doc.id == id) else {
            return false;
        };
        if self.docs[index].path.is_none() {
            return self.save_document_as(id);
        }
        let path = self.docs[index].path.clone().unwrap();
        let result = std::fs::write(&path, &self.docs[index].text);
        match result {
            Ok(()) => {
                self.docs[index].dirty = false;
                self.docs[index].stamp = FileStamp::read(&path);
                self.status = format!("Gespeichert: {}", display_path(&path));
                true
            }
            Err(err) => {
                self.status = format!("Fehler beim Speichern: {err}");
                false
            }
        }
    }

    fn save_document_as(&mut self, id: u64) -> bool {
        let Some(index) = self.docs.iter().position(|doc| doc.id == id) else {
            return false;
        };
        let kind = self.docs[index].kind;
        let suggested = self.docs[index]
            .file_name()
            .unwrap_or_else(|| kind.default_file_name().to_owned());
        let mut dialog = rfd::FileDialog::new()
            .add_filter(kind.filter_name(), kind.extensions())
            .set_file_name(suggested);
        if let Some(dir) = self.docs[index]
            .path
            .as_deref()
            .and_then(Path::parent)
            .or(self.root.as_deref())
        {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else {
            return false;
        };
        let has_supported_extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| DocKind::from_extension(e).is_some())
            .unwrap_or(false);
        if !has_supported_extension {
            path.set_extension(kind.extension());
        }

        let text = self.docs[index].text.clone();
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.docs[index].stamp = FileStamp::read(&path);
                let doc = &mut self.docs[index];
                doc.path = Some(path.clone());
                doc.untitled_name = None;
                doc.dirty = false;
                self.status = format!("Gespeichert: {}", display_path(&path));
            }
            Err(err) => {
                self.status = format!("Fehler beim Speichern: {err}");
                return false;
            }
        }
        self.push_recent(path.clone());
        if self.root.is_none() {
            self.root = path.parent().map(Path::to_path_buf);
        }
        self.refresh_files();
        true
    }

    fn save_active(&mut self) {
        if let Some(id) = self.active {
            self.save_document(id);
        }
    }

    fn save_active_as(&mut self) {
        if let Some(id) = self.active {
            self.save_document_as(id);
        }
    }

    fn save_documents(&mut self, ids: &[u64]) -> bool {
        for id in ids {
            if !self.save_document(*id) {
                return false;
            }
        }
        true
    }

    /// Liest die aktive Datei erneut von der Festplatte ein.
    fn reload_active(&mut self) {
        if let Some(id) = self.active {
            self.reload_document(id);
        }
    }

    /// Liest ein Dokument erneut von der Festplatte ein.
    fn reload_document(&mut self, id: u64) {
        let Some(index) = self.docs.iter().position(|doc| doc.id == id) else {
            return;
        };
        let Some(path) = self.docs[index].path.clone() else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let doc = &mut self.docs[index];
                doc.text = text;
                doc.dirty = false;
                doc.stamp = FileStamp::read(&path);
                doc.reset_view_state();
                self.status = format!("Neu geladen: {}", display_path(&path));
            }
            Err(err) => {
                self.status = format!("Fehler beim Laden: {err}");
            }
        }
    }

    fn reload_possible(&self) -> bool {
        self.active_doc()
            .map(|doc| doc.path.is_some() && !doc.dirty)
            .unwrap_or(false)
    }

    /// Prüft geöffnete Dateien regelmäßig auf Änderungen von außerhalb.
    ///
    /// Ungespeicherte Dokumente bleiben unangetastet; dort wird ein Dialog
    /// angeboten. Dokumente ohne eigene Änderungen werden direkt neu geladen.
    fn check_external_changes(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(last) = self.last_watch {
            if now.duration_since(last) < FILE_WATCH_INTERVAL {
                return;
            }
        }
        self.last_watch = Some(now);

        let mut reloaded: Vec<PathBuf> = Vec::new();
        let mut conflicts: Vec<(u64, PathBuf)> = Vec::new();
        let mut missing: Vec<PathBuf> = Vec::new();

        for index in 0..self.docs.len() {
            let Some(path) = self.docs[index].path.clone() else {
                continue;
            };
            let Some(stamp) = FileStamp::read(&path) else {
                // Datei wurde gelöscht oder ist nicht mehr erreichbar.
                if self.docs[index].stamp.take().is_some() {
                    missing.push(path);
                }
                continue;
            };
            if self.docs[index].stamp == Some(stamp) {
                continue;
            }
            // Erst lesen, dann den Stand übernehmen: Schlägt das Lesen fehl
            // (z. B. mitten im Schreiben), wird es beim nächsten Mal erneut versucht.
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            self.docs[index].stamp = Some(stamp);
            if text == self.docs[index].text {
                continue;
            }
            let doc = &mut self.docs[index];
            if doc.dirty {
                conflicts.push((doc.id, path));
            } else {
                doc.text = text;
                doc.reset_view_state();
                reloaded.push(path);
            }
        }

        if let Some(path) = reloaded.last() {
            self.status = format!("Extern geändert und neu geladen: {}", display_path(path));
        }
        if let Some(path) = missing.last() {
            self.status = format!(
                "Datei wurde extern gelöscht oder verschoben: {}",
                display_path(path)
            );
        }
        for (id, path) in conflicts {
            if !self.reload_requests.contains(&id) {
                self.reload_requests.push(id);
            }
            self.status = format!(
                "Extern geändert (ungespeicherte Änderungen): {}",
                display_path(&path)
            );
        }
        if !self.reload_requests.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// Zeigt die aktive Datei im Dateimanager an.
    fn reveal_active(&mut self) {
        let Some(path) = self.active_doc().and_then(|doc| doc.path.clone()) else {
            return;
        };
        self.status = if system::reveal(&path) {
            format!("Im Explorer angezeigt: {}", display_path(&path))
        } else {
            "Datei konnte nicht im Explorer angezeigt werden".to_owned()
        };
    }

    /// Registriert den Editor für „Öffnen mit“ im Kontextmenü.
    fn register_open_with(&mut self) {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(_) => {
                self.status = "Registrierung für „Öffnen mit“ fehlgeschlagen".to_owned();
                return;
            }
        };
        self.status = if system::register_open_with(&exe) {
            "Anwendung für „Öffnen mit“ registriert".to_owned()
        } else {
            "Registrierung für „Öffnen mit“ fehlgeschlagen".to_owned()
        };
    }

    // -- Schließen ----------------------------------------------------------

    fn docs_for_target(&self, target: CloseTarget) -> Vec<u64> {
        match target {
            CloseTarget::Document(id) => vec![id],
            CloseTarget::Folder => {
                let Some(root) = &self.root else {
                    return Vec::new();
                };
                self.docs
                    .iter()
                    .filter(|doc| {
                        doc.path
                            .as_deref()
                            .map(|path| path.starts_with(root))
                            .unwrap_or(false)
                    })
                    .map(|doc| doc.id)
                    .collect()
            }
            CloseTarget::Window => self.docs.iter().map(|doc| doc.id).collect(),
        }
    }

    fn request_close(&mut self, target: CloseTarget) {
        let ids = self.docs_for_target(target);
        let dirty = ids.iter().any(|id| {
            self.docs
                .iter()
                .find(|doc| doc.id == *id)
                .map(|doc| doc.dirty)
                .unwrap_or(false)
        });
        match target {
            CloseTarget::Document(id) if !dirty => self.close_document(id),
            CloseTarget::Folder if !dirty => self.close_folder_now(),
            CloseTarget::Window if !dirty => self.close_window_now(),
            _ => self.pending_close = Some(target),
        }
    }

    fn execute_close(&mut self, ctx: &egui::Context, target: CloseTarget) {
        match target {
            CloseTarget::Document(id) => self.close_document(id),
            CloseTarget::Folder => self.close_folder_now(),
            CloseTarget::Window => {
                self.close_window_now();
                let _ = ctx;
            }
        }
    }

    fn close_document(&mut self, id: u64) {
        let Some(index) = self.docs.iter().position(|doc| doc.id == id) else {
            return;
        };
        self.docs.remove(index);
        if self.active == Some(id) {
            self.active = self
                .docs
                .get(index)
                .or_else(|| self.docs.last())
                .map(|doc| doc.id);
        }
        self.status = "Dokument geschlossen".to_owned();
    }

    fn close_folder_now(&mut self) {
        let ids = self.docs_for_target(CloseTarget::Folder);
        self.docs.retain(|doc| !ids.contains(&doc.id));
        if let Some(active) = self.active {
            if !self.docs.iter().any(|doc| doc.id == active) {
                self.active = self.docs.last().map(|doc| doc.id);
            }
        }
        self.files.clear();
        self.root = None;
        self.status = "Ordner geschlossen".to_owned();
    }

    fn close_window_now(&mut self) {
        self.window_close_allowed = true;
        self.docs.iter_mut().for_each(|doc| doc.dirty = false);
    }

    // -- Export -------------------------------------------------------------

    fn open_export_dialog(&mut self) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        if doc.kind != DocKind::Markdown {
            self.status = "HTML-Export ist nur für Markdown-Dokumente verfügbar".to_owned();
            return;
        }
        let doc_id = doc.id;
        let title = doc
            .file_name()
            .and_then(|name| name.rsplit_once('.').map(|(stem, _)| stem.to_owned()))
            .unwrap_or_else(|| "Unbenannt".to_owned());
        self.export = Some(ExportState {
            doc_id,
            options: ExportOptions {
                full_document: true,
                css: CssChoice::Default,
                title,
            },
        });
    }

    fn run_export(&mut self) -> bool {
        let Some(state) = &self.export else {
            return false;
        };
        let doc_id = state.doc_id;
        let options = state.options.clone();
        let Some(index) = self.docs.iter().position(|doc| doc.id == doc_id) else {
            return false;
        };
        let suggested = format!(
            "{}.html",
            self.docs[index]
                .file_name()
                .and_then(|name| name.rsplit_once('.').map(|(stem, _)| stem.to_owned()))
                .unwrap_or_else(|| "Unbenannt".to_owned())
        );
        let mut dialog = rfd::FileDialog::new()
            .add_filter("HTML", &["html", "htm"])
            .set_file_name(suggested);
        if let Some(dir) = self.docs[index]
            .path
            .as_deref()
            .and_then(Path::parent)
            .or(self.root.as_deref())
        {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("html");
        }
        let html = md2html::build_html(&self.docs[index].text, &options, &path);
        match std::fs::write(&path, html) {
            Ok(()) => {
                self.status = format!("HTML exportiert: {}", display_path(&path));
                true
            }
            Err(err) => {
                self.status = format!("Fehler beim Export: {err}");
                false
            }
        }
    }

    // -- Design / Titel -----------------------------------------------------

    fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(self.theme.preference());
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.active_doc() {
            Some(doc) => format!(
                "{}{} – {APP_NAME}",
                doc.title(),
                if doc.dirty { " •" } else { "" }
            ),
            None => format!("Unbenannt – {APP_NAME}"),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    // -- Tastenkürzel -------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)) {
            self.save_active_as();
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save_active();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            self.pick_file(ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::N)) {
            self.new_document(ctx, DocKind::Markdown);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::W)) {
            if let Some(id) = self.active {
                self.request_close(CloseTarget::Document(id));
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            if let Some(doc) = self.active_doc_mut() {
                doc.find_open = true;
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::H)) {
            if let Some(doc) = self.active_doc_mut() {
                doc.find_open = true;
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z)) {
            if let Some(doc) = self.active_doc_mut() {
                if editor::undo(ctx, doc) {
                    self.status = "Rückgängig".to_owned();
                }
            }
        }
        if ctx.input_mut(|i| {
            i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)
                || i.consume_key(Modifiers::COMMAND, Key::Y)
        }) {
            if let Some(doc) = self.active_doc_mut() {
                if editor::redo(ctx, doc) {
                    self.status = "Wiederholt".to_owned();
                }
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::B)) {
            self.format_active(ctx, FormatAction::Bold);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::I)) {
            self.format_active(ctx, FormatAction::Italic);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::K)) {
            self.format_active(ctx, FormatAction::Link);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num1)) {
            self.format_active(ctx, FormatAction::Heading1);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num2)) {
            self.format_active(ctx, FormatAction::Heading2);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Num3)) {
            self.format_active(ctx, FormatAction::Heading3);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F)) {
            self.format_active_html(ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F5)) {
            if self.reload_possible() {
                self.reload_active();
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F11)) {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }

        // Nächster/vorheriger Tab
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Tab)) {
            self.cycle_tab(1);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Tab)) {
            self.cycle_tab(-1);
        }
    }

    fn cycle_tab(&mut self, direction: i32) {
        if self.docs.len() < 2 {
            return;
        }
        let Some(index) = self.active_index() else {
            return;
        };
        let len = self.docs.len() as i32;
        let next = (index as i32 + direction).rem_euclid(len) as usize;
        self.active = Some(self.docs[next].id);
    }

    fn format_active(&mut self, ctx: &egui::Context, action: FormatAction) {
        if let Some(id) = self.active {
            if let Some(index) = self.docs.iter().position(|doc| doc.id == id) {
                let mut doc = self.docs.remove(index);
                if doc.kind == DocKind::Markdown {
                    editor::apply_format(ctx, &mut doc, action, &mut self.status);
                }
                self.docs.insert(index, doc);
            }
        }
    }

    fn format_active_html(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.active {
            if let Some(index) = self.docs.iter().position(|doc| doc.id == id) {
                let mut doc = self.docs.remove(index);
                if doc.kind == DocKind::Html {
                    editor::format_document(ctx, &mut doc, &mut self.status);
                }
                self.docs.insert(index, doc);
            }
        }
    }

    // -- Drag & Drop --------------------------------------------------------

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let mut supported = 0usize;
        let mut skipped = 0usize;
        for file in dropped {
            let Some(path) = file.path else {
                continue;
            };
            if is_supported(&path) {
                self.open_path(ctx, path);
                supported += 1;
            } else {
                skipped += 1;
            }
        }
        if supported > 0 {
            self.status = format!("{supported} Datei(en) geöffnet");
        }
        if skipped > 0 {
            self.status = format!("{supported} geöffnet, {skipped} nicht unterstützt");
        }
    }

    fn draw_drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|input| !input.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop")));
        let rect = ctx.screen_rect();
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(90));
        painter.rect_stroke(
            rect.shrink(18.0),
            12.0,
            egui::Stroke::new(2.0_f32, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Dateien zum Öffnen ablegen",
            FontId::proportional(22.0),
            Color32::WHITE,
        );
    }

    // -- Fenster schließen --------------------------------------------------

    fn handle_window_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.window_close_allowed && self.docs.iter().any(|doc| doc.dirty) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_close = Some(CloseTarget::Window);
            }
        }
    }

    // -- Hauptoberfläche ----------------------------------------------------

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                self.format_menu(ui);
                self.view_menu(ui);
                self.help_menu(ui);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(self.theme.label()).frame(false))
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
                        .add_enabled(
                            self.has_document(),
                            egui::Button::new("Speichern").frame(false),
                        )
                        .on_hover_text("Speichern (Strg+S)")
                        .clicked()
                    {
                        self.save_active();
                    }
                    if ui
                        .add(egui::Button::new("Öffnen").frame(false))
                        .on_hover_text("Datei öffnen (Strg+O)")
                        .clicked()
                    {
                        self.pick_file(ctx);
                    }
                });
            });
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Datei", |ui| {
            let ctx = ui.ctx().clone();
            if menu_item(ui, "Neu (Markdown)", Some("Strg+N")) {
                self.new_document(&ctx, DocKind::Markdown);
                ui.close();
            }
            if menu_item(ui, "Neu (HTML)", None) {
                self.new_document(&ctx, DocKind::Html);
                ui.close();
            }

            ui.separator();

            if menu_item(ui, "Datei öffnen …", Some("Strg+O")) {
                self.pick_file(&ctx);
                ui.close();
            }
            if menu_item(ui, "Ordner öffnen …", None) {
                self.pick_folder(&ctx);
                ui.close();
            }

            ui.separator();

            let has_doc = self.has_document();
            if ui
                .add_enabled(
                    has_doc,
                    egui::Button::new("Speichern").shortcut_text("Strg+S"),
                )
                .clicked()
            {
                self.save_active();
                ui.close();
            }
            if ui
                .add_enabled(
                    has_doc,
                    egui::Button::new("Speichern unter …").shortcut_text("Strg+Umschalt+S"),
                )
                .clicked()
            {
                self.save_active_as();
                ui.close();
            }
            if ui
                .add_enabled(
                    self.reload_possible(),
                    egui::Button::new("Neu laden").shortcut_text("F5"),
                )
                .clicked()
            {
                self.reload_active();
                ui.close();
            }
            let can_export = self
                .active_doc()
                .map(|doc| doc.kind == DocKind::Markdown && !doc.text.trim().is_empty())
                .unwrap_or(false);
            if ui
                .add_enabled(can_export, egui::Button::new("HTML exportieren …"))
                .clicked()
            {
                self.open_export_dialog();
                ui.close();
            }

            ui.separator();

            if ui
                .add_enabled(
                    has_doc,
                    egui::Button::new("Datei schließen").shortcut_text("Strg+W"),
                )
                .clicked()
            {
                if let Some(id) = self.active {
                    self.request_close(CloseTarget::Document(id));
                }
                ui.close();
            }
            if ui
                .add_enabled(self.root.is_some(), egui::Button::new("Ordner schließen"))
                .clicked()
            {
                self.request_close(CloseTarget::Folder);
                ui.close();
            }

            ui.separator();

            let has_path = self
                .active_doc()
                .map(|doc| doc.path.is_some())
                .unwrap_or(false);
            if ui
                .add_enabled(
                    has_path,
                    egui::Button::new("Im Explorer zeigen"),
                )
                .clicked()
            {
                self.reveal_active();
                ui.close();
            }
            if ui
                .button("Mit „Öffnen mit“ verknüpfen")
                .on_hover_text(
                    "Registriert den Editor für Markdown- und HTML-Dateien im Kontextmenü „Öffnen mit“",
                )
                .clicked()
            {
                self.register_open_with();
                ui.close();
            }

            ui.separator();

            ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
                ui.menu_button("Zuletzt geöffnet", |ui| {
                    let recent = self.recent.clone();
                    for path in recent {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| display_path(&path));
                        if ui
                            .add(egui::Button::new(name))
                            .on_hover_text(display_path(&path))
                            .clicked()
                        {
                            self.open_path(ui.ctx(), path);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if menu_item(ui, "Liste leeren", None) {
                        self.recent.clear();
                        ui.close();
                    }
                });
            });

            ui.separator();

            if menu_item(ui, "Beenden", None) {
                self.request_close(CloseTarget::Window);
            }
        });
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Bearbeiten", |ui| {
            let ctx = ui.ctx().clone();
            let has_document = self.has_document();

            if ui
                .add_enabled(
                    has_document,
                    egui::Button::new("Rückgängig").shortcut_text("Strg+Z"),
                )
                .clicked()
            {
                if let Some(doc) = self.active_doc_mut() {
                    if editor::undo(&ctx, doc) {
                        self.status = "Rückgängig".to_owned();
                    }
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    has_document,
                    egui::Button::new("Wiederholen").shortcut_text("Strg+Umschalt+Z"),
                )
                .clicked()
            {
                if let Some(doc) = self.active_doc_mut() {
                    if editor::redo(&ctx, doc) {
                        self.status = "Wiederholt".to_owned();
                    }
                }
                ui.close();
            }

            ui.separator();

            if ui
                .add_enabled(
                    has_document,
                    egui::Button::new("Ausschneiden").shortcut_text("Strg+X"),
                )
                .clicked()
            {
                if let Some(doc) = self.active_doc() {
                    editor::focus(&ctx, doc);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut);
                ui.close();
            }
            if ui
                .add_enabled(
                    has_document,
                    egui::Button::new("Kopieren").shortcut_text("Strg+C"),
                )
                .clicked()
            {
                if let Some(doc) = self.active_doc() {
                    editor::focus(&ctx, doc);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy);
                ui.close();
            }
            if ui
                .add_enabled(
                    has_document,
                    egui::Button::new("Einfügen").shortcut_text("Strg+V"),
                )
                .clicked()
            {
                if let Some(doc) = self.active_doc() {
                    editor::focus(&ctx, doc);
                }
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
                if let Some(doc) = self.active_doc() {
                    editor::select_all(&ctx, doc);
                }
                ui.close();
            }

            ui.separator();

            if ui
                .add_enabled(
                    has_document,
                    egui::Button::new("Suchen …").shortcut_text("Strg+F"),
                )
                .clicked()
            {
                if let Some(doc) = self.active_doc_mut() {
                    doc.find_open = true;
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    has_document,
                    egui::Button::new("Ersetzen …").shortcut_text("Strg+H"),
                )
                .clicked()
            {
                if let Some(doc) = self.active_doc_mut() {
                    doc.find_open = true;
                }
                ui.close();
            }
        });
    }

    fn format_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Format", |ui| {
            let ctx = ui.ctx().clone();
            let kind = self.active_doc().map(|doc| doc.kind);
            match kind {
                Some(DocKind::Markdown) => {
                    format_item(ui, self, &ctx, "Fett", Some("Strg+B"), FormatAction::Bold);
                    format_item(ui, self, &ctx, "Kursiv", Some("Strg+I"), FormatAction::Italic);
                    format_item(
                        ui,
                        self,
                        &ctx,
                        "Durchgestrichen",
                        None,
                        FormatAction::Strikethrough,
                    );
                    format_item(ui, self, &ctx, "Inline-Code", None, FormatAction::InlineCode);
                    format_item(ui, self, &ctx, "Codeblock", None, FormatAction::CodeBlock);

                    ui.separator();

                    format_item(
                        ui,
                        self,
                        &ctx,
                        "Überschrift 1",
                        Some("Strg+1"),
                        FormatAction::Heading1,
                    );
                    format_item(
                        ui,
                        self,
                        &ctx,
                        "Überschrift 2",
                        Some("Strg+2"),
                        FormatAction::Heading2,
                    );
                    format_item(
                        ui,
                        self,
                        &ctx,
                        "Überschrift 3",
                        Some("Strg+3"),
                        FormatAction::Heading3,
                    );

                    ui.separator();

                    format_item(
                        ui,
                        self,
                        &ctx,
                        "Aufzählungsliste",
                        None,
                        FormatAction::BulletList,
                    );
                    format_item(
                        ui,
                        self,
                        &ctx,
                        "Nummerierte Liste",
                        None,
                        FormatAction::NumberedList,
                    );
                    format_item(ui, self, &ctx, "Zitat", None, FormatAction::Quote);
                    format_item(ui, self, &ctx, "Tabelle", None, FormatAction::Table);

                    ui.separator();

                    format_item(ui, self, &ctx, "Link", Some("Strg+K"), FormatAction::Link);
                    format_item(ui, self, &ctx, "Bild einfügen …", None, FormatAction::Image);
                    format_item(ui, self, &ctx, "Trennlinie", None, FormatAction::Rule);
                }
                Some(DocKind::Html) => {
                    if ui
                        .add(
                            egui::Button::new("HTML formatieren")
                                .shortcut_text("Strg+Umschalt+F"),
                        )
                        .clicked()
                    {
                        self.format_active_html(&ctx);
                        ui.close();
                    }
                }
                None => {
                    ui.add_enabled(false, egui::Button::new("Kein Dokument geöffnet"));
                }
            }
        });
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Ansicht", |ui| {
            if ui.checkbox(&mut self.show_files, "Dateiliste").clicked() {
                ui.close();
            }

            ui.separator();

            if ui
                .selectable_value(&mut self.view, ViewMode::Split, "Geteilte Ansicht")
                .clicked()
            {
                ui.close();
            }
            if ui
                .selectable_value(&mut self.view, ViewMode::Editor, "Nur Editor")
                .clicked()
            {
                ui.close();
            }
            if ui
                .selectable_value(&mut self.view, ViewMode::Preview, "Nur Vorschau")
                .clicked()
            {
                ui.close();
            }

            ui.separator();

            ui.checkbox(&mut self.sync_scroll, "Synchronisiertes Scrollen");
            ui.checkbox(&mut self.line_numbers, "Zeilennummern");
            ui.checkbox(&mut self.auto_indent, "Automatische Einrückung");
            ui.checkbox(&mut self.syntax_highlighting, "Syntaxhervorhebung");

            ui.separator();

            ui.menu_button("Design", |ui| {
                if ui
                    .selectable_value(&mut self.theme, ThemeMode::System, "◐  System")
                    .clicked()
                {
                    ui.close();
                }
                if ui
                    .selectable_value(&mut self.theme, ThemeMode::Light, "☀  Hell")
                    .clicked()
                {
                    ui.close();
                }
                if ui
                    .selectable_value(&mut self.theme, ThemeMode::Dark, "☾  Dunkel")
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

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Hilfe", |ui| {
            if menu_item(ui, "⌨  Tastenkürzel", None) {
                self.shortcuts_open = true;
                ui.close();
            }
            if menu_item(ui, "ℹ  Über MD Editor", None) {
                self.about_open = true;
                ui.close();
            }
        });
    }

    fn tab_bar(&mut self, ctx: &egui::Context) {
        if self.docs.is_empty() {
            return;
        }
        let mut activate = None;
        let mut close = None;
        let mut close_others = None;
        let mut save = None;

        egui::TopBottomPanel::top("tab_bar")
            .exact_height(34.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("tab_scroll")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for doc in &self.docs {
                                    let selected = self.active == Some(doc.id);
                                    let label = format!(
                                        "{}  {}{}",
                                        doc.kind.badge(),
                                        doc.title(),
                                        if doc.dirty { " •" } else { "" }
                                    );
                                    let response = ui
                                        .selectable_label(selected, RichText::new(label))
                                        .on_hover_text(
                                            doc.path
                                                .as_ref()
                                                .map(|p| display_path(p))
                                                .unwrap_or_else(|| {
                                                    "Noch nicht gespeichert".to_owned()
                                                }),
                                        );
                                    if response.clicked() {
                                        activate = Some(doc.id);
                                    }
                                    response.context_menu(|ui| {
                                        if ui.button("Speichern").clicked() {
                                            save = Some(doc.id);
                                            ui.close();
                                        }
                                        if ui.button("Schließen").clicked() {
                                            close = Some(doc.id);
                                            ui.close();
                                        }
                                        if ui.button("Andere schließen").clicked() {
                                            close_others = Some(doc.id);
                                            ui.close();
                                        }
                                    });
                                    if ui
                                        .add(
                                            egui::Button::new("×")
                                                .frame(false)
                                                .small(),
                                        )
                                        .on_hover_text("Tab schließen")
                                        .clicked()
                                    {
                                        close = Some(doc.id);
                                    }
                                }
                            });
                        });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button("+", |ui| {
                            if ui.button("Neue Markdown-Datei").clicked() {
                                self.new_document(ctx, DocKind::Markdown);
                                ui.close();
                            }
                            if ui.button("Neue HTML-Datei").clicked() {
                                self.new_document(ctx, DocKind::Html);
                                ui.close();
                            }
                        });
                    });
                });
            });

        if let Some(id) = activate {
            self.active = Some(id);
        }
        if let Some(id) = save {
            self.active = Some(id);
            self.save_document(id);
        }
        if let Some(id) = close {
            self.request_close(CloseTarget::Document(id));
        }
        if let Some(keep) = close_others {
            let ids: Vec<u64> = self
                .docs
                .iter()
                .filter(|doc| doc.id != keep && !doc.dirty)
                .map(|doc| doc.id)
                .collect();
            for id in ids {
                self.close_document(id);
            }
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.small(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(doc) = self.active_doc() {
                        let chars = doc.text.chars().count();
                        let words = doc.text.split_whitespace().count();
                        let lines = doc.text.lines().count().max(1);
                        ui.small(format!(
                            "{lines} Zeilen · {words} Wörter · {chars} Zeichen"
                        ));
                        ui.separator();
                        ui.small(editor::status_text(doc));
                        ui.separator();
                        ui.small(doc.kind.editor_title());
                    }
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
                            .small_button("Ordner")
                            .on_hover_text("Ordner öffnen")
                            .clicked()
                        {
                            self.pick_folder(ctx);
                        }
                        if ui
                            .add_enabled(
                                self.root.is_some(),
                                egui::Button::new("×").frame(false).small(),
                            )
                            .on_hover_text("Ordner schließen")
                            .clicked()
                        {
                            self.request_close(CloseTarget::Folder);
                        }
                    });
                });

                match &self.root {
                    Some(root) => {
                        let name = root
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| display_path(root));
                        ui.weak(name).on_hover_text(display_path(root));
                    }
                    None => {
                        ui.weak("Kein Ordner geöffnet");
                    }
                }

                ui.separator();

                if self.files.is_empty() {
                    ui.add_space(6.0);
                    ui.weak(if self.root.is_some() {
                        "Keine Markdown- oder HTML-Dateien gefunden."
                    } else {
                        "Öffne einen Ordner, um deine Dateien zu sehen."
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("file_scroll")
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
                                let selected = self
                                    .active_doc()
                                    .and_then(|doc| doc.path.as_deref())
                                    == Some(file.as_path());
                                let kind = DocKind::from_path(file);
                                let text = format!("[{}] {label}", kind.badge());
                                if ui.selectable_label(selected, text).clicked() {
                                    clicked = Some(file.clone());
                                }
                            }
                        });
                }
            });

        if let Some(path) = clicked {
            self.open_path(ctx, path);
        }
    }

    fn editor_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (name, dirty, kind, path, has_doc) = match self.active_doc() {
                Some(doc) => (
                    doc.title(),
                    doc.dirty,
                    Some(doc.kind),
                    doc.path.clone(),
                    true,
                ),
                None => ("Keine Datei".to_owned(), false, None, None, false),
            };

            if let Some(kind) = kind {
                ui.label(
                    RichText::new(kind.badge())
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
            let title = ui.strong(name);
            if let Some(path) = &path {
                title.on_hover_text(display_path(path));
            }
            if dirty {
                ui.colored_label(ui.visuals().warn_fg_color, "•")
                    .on_hover_text("Ungespeicherte Änderungen");
            }

            if ui
                .add_enabled(has_doc, egui::Button::new("×").frame(false).small())
                .on_hover_text("Datei schließen (Strg+W)")
                .clicked()
            {
                if let Some(id) = self.active {
                    self.request_close(CloseTarget::Document(id));
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut self.view, ViewMode::Preview, "Vorschau");
                ui.selectable_value(&mut self.view, ViewMode::Editor, "Editor");
                ui.selectable_value(&mut self.view, ViewMode::Split, "Geteilt");
            });
        });
    }

    fn central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.editor_toolbar(ui);
            ui.separator();

            let Some(index) = self.active_index() else {
                self.empty_state(ui);
                return;
            };

            let mut doc = self.docs.remove(index);
            match self.view {
                ViewMode::Split => {
                    let preview_width = (ui.available_width() * 0.5).clamp(220.0, 1200.0);
                    egui::SidePanel::right("preview_panel")
                        .resizable(true)
                        .default_width(preview_width)
                        .width_range(180.0..=1200.0)
                        .show_inside(ui, |ui| {
                            self.preview_pane(ui, &mut doc);
                        });
                    self.editor_pane(ui, &mut doc);
                }
                ViewMode::Editor => self.editor_pane(ui, &mut doc),
                ViewMode::Preview => self.preview_pane(ui, &mut doc),
            }
            self.docs.insert(index, doc);
        });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.heading("Keine Datei geöffnet");
            ui.add_space(8.0);
            ui.weak("Erstelle ein neues Dokument oder öffne eine Markdown- bzw. HTML-Datei.");
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if ui.button("Neues Markdown-Dokument").clicked() {
                    self.new_document(&ctx, DocKind::Markdown);
                }
                if ui.button("Neues HTML-Dokument").clicked() {
                    self.new_document(&ctx, DocKind::Html);
                }
                if ui.button("Datei öffnen …").clicked() {
                    self.pick_file(&ctx);
                }
            });
            ui.add_space(24.0);
            ui.weak("Dateien können auch per Drag & Drop in das Fenster gezogen werden.");
        });
    }

    fn editor_pane(&mut self, ui: &mut egui::Ui, doc: &mut Document) {
        ui.horizontal(|ui| {
            ui.strong(format!("✏  {}", doc.kind.editor_title()));
            if doc.dirty {
                ui.colored_label(ui.visuals().warn_fg_color, "ungespeichert");
            }
        });
        ui.add_space(2.0);

        let options = EditorOptions {
            line_numbers: self.line_numbers,
            syntax_highlighting: self.syntax_highlighting,
            auto_indent: self.auto_indent,
            sync_scroll: self.sync_scroll,
        };
        editor::show_editor(ui, doc, &mut self.status, &options);
    }

    fn preview_pane(&mut self, ui: &mut egui::Ui, doc: &mut Document) {
        ui.horizontal(|ui| {
            ui.strong("Vorschau");
            ui.weak(format!("({})", doc.kind.filter_name()));
        });
        ui.add_space(2.0);

        let base_dir = doc.base_dir().map(Path::to_path_buf);
        let incoming_offset = if self.sync_scroll && doc.scroll_master == ScrollMaster::Editor {
            Some(doc.sync_ratio * doc.preview_max)
        } else {
            None
        };

        let mut area = egui::ScrollArea::vertical()
            .id_salt(("preview_scroll", doc.id))
            .auto_shrink([false, false]);
        if let Some(offset) = incoming_offset {
            area = area.vertical_scroll_offset(offset);
        }

        let output = area.show(ui, |ui| match doc.kind {
            DocKind::Markdown => {
                if doc.text.trim().is_empty() {
                    ui.weak("Die Vorschau erscheint hier.");
                } else {
                    let image_base = base_dir
                        .as_deref()
                        .and_then(|path| url::Url::from_directory_path(path).ok());
                    let viewer = match image_base {
                        Some(base) => {
                            CommonMarkViewer::new().default_implicit_uri_scheme(base.to_string())
                        }
                        None => CommonMarkViewer::new(),
                    };
                    viewer.show(ui, &mut self.cache, &doc.text);
                }
            }
            DocKind::Html => {
                if doc.text.trim().is_empty() {
                    ui.weak("Die HTML-Vorschau erscheint hier.");
                } else {
                    crate::html_preview::render(
                        ui,
                        &doc.text,
                        base_dir.as_deref(),
                        &mut self.images,
                    );
                }
            }
        });

        let scroll_offset = output.state.offset.y;
        let applied = incoming_offset.unwrap_or(doc.preview_offset);
        if (scroll_offset - applied).abs() > 0.5 {
            doc.preview_offset = scroll_offset;
            doc.preview_max = (output.content_size.y - output.inner_rect.height()).max(1.0);
            doc.sync_ratio = (scroll_offset / doc.preview_max).clamp(0.0, 1.0);
            doc.scroll_master = ScrollMaster::Preview;
        }
    }

    // -- Dialoge ------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.about_dialog(ctx);
        self.shortcuts_dialog(ctx);
        self.export_dialog(ctx);
        self.close_dialog(ctx);
        self.reload_dialog(ctx);
    }

    fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("about_modal")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading(APP_NAME);
            ui.add_space(4.0);
            ui.label("Markdown- und HTML-Editor mit Live-Vorschau.");
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
                    ui.label("egui_commonmark · pulldown-cmark");
                    ui.end_row();
                    ui.weak("HTML");
                    ui.label("tl");
                    ui.end_row();
                    ui.weak("Plattformen");
                    ui.label("Windows x86-64, Linux x86-64");
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

    fn shortcuts_dialog(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_open {
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("shortcuts_modal")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("Tastenkürzel");
            ui.add_space(8.0);
            egui::Grid::new("shortcut_grid")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .striped(true)
                .show(ui, |ui| {
                    let shortcuts = [
                        ("Strg+N", "Neues Markdown-Dokument"),
                        ("Strg+O", "Datei öffnen"),
                        ("Strg+S", "Speichern"),
                        ("Strg+Umschalt+S", "Speichern unter"),
                        ("Strg+W", "Datei schließen"),
                        ("Strg+Tab", "Nächster Tab"),
                        ("Strg+B", "Fett"),
                        ("Strg+I", "Kursiv"),
                        ("Strg+K", "Link einfügen"),
                        ("Strg+1/2/3", "Überschrift 1/2/3"),
                        ("Strg+F", "Suchen"),
                        ("Strg+H", "Suchen und Ersetzen"),
                        ("Strg+Z", "Rückgängig"),
                        ("Strg+Umschalt+Z", "Wiederholen"),
                        ("Strg+Umschalt+F", "HTML formatieren"),
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

    fn export_dialog(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.export else {
            return;
        };
        let mut close = false;
        let mut do_export = false;
        let modal = egui::Modal::new(egui::Id::new("export_modal")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.heading("HTML exportieren");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label("Titel:");
                ui.add(
                    TextEdit::singleline(&mut state.options.title)
                        .desired_width(f32::INFINITY)
                        .hint_text("Titel des HTML-Dokuments"),
                );
            });
            ui.add_space(6.0);
            ui.checkbox(
                &mut state.options.full_document,
                "Vollständiges HTML-Dokument (Head und Body)",
            );
            ui.label(
                RichText::new(
                    "Ohne vollständiges Dokument wird nur das HTML-Fragment exportiert.",
                )
                .small()
                .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(8.0);
            ui.label("Stylesheet:");
            ui.radio_value(
                &mut state.options.css,
                CssChoice::Default,
                CssChoice::Default.label(),
            );
            ui.radio_value(
                &mut state.options.css,
                CssChoice::None,
                CssChoice::None.label(),
            );
            let is_file = matches!(state.options.css, CssChoice::File(_));
            ui.horizontal(|ui| {
                let response = ui.radio(is_file, "Eigene CSS-Datei einbinden");
                if response.clicked() && !is_file {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSS", &["css"])
                        .pick_file()
                    {
                        state.options.css = CssChoice::File(path);
                    }
                }
                if let CssChoice::File(path) = &state.options.css {
                    ui.weak(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                }
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Exportieren …").clicked() {
                    do_export = true;
                }
                if ui.button("Abbrechen").clicked() {
                    close = true;
                }
            });
        });

        if do_export {
            if self.run_export() {
                close = true;
            }
        }
        if close || modal.should_close() {
            self.export = None;
        }
    }

    fn close_dialog(&mut self, ctx: &egui::Context) {
        let Some(target) = self.pending_close else {
            return;
        };
        let ids = self.docs_for_target(target);
        let dirty: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| {
                self.docs
                    .iter()
                    .find(|doc| doc.id == *id)
                    .map(|doc| doc.dirty)
                    .unwrap_or(false)
            })
            .collect();
        let label = match target {
            CloseTarget::Document(_) => {
                let name = dirty
                    .first()
                    .and_then(|id| self.docs.iter().find(|doc| doc.id == *id))
                    .map(|doc| doc.title())
                    .unwrap_or_else(|| "Dokument".to_owned());
                format!("Das Dokument „{name}“ enthält ungespeicherte Änderungen.")
            }
            CloseTarget::Folder => format!(
                "{} Dokument(e) im geöffneten Ordner enthalten ungespeicherte Änderungen.",
                dirty.len()
            ),
            CloseTarget::Window => format!(
                "{} Dokument(e) enthalten ungespeicherte Änderungen.",
                dirty.len()
            ),
        };

        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("close_modal")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading("Ungespeicherte Änderungen");
            ui.add_space(8.0);
            ui.label(label);
            ui.add_space(6.0);
            ui.label("Was möchtest du tun?");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Speichern").clicked() {
                    save = true;
                }
                if ui.button("Verwerfen").clicked() {
                    discard = true;
                }
                if ui.button("Abbrechen").clicked() {
                    cancel = true;
                }
            });
        });

        if save {
            if self.save_documents(&dirty) {
                self.pending_close = None;
                self.execute_close(ctx, target);
            }
        } else if discard {
            for id in &dirty {
                if let Some(doc) = self.docs.iter_mut().find(|doc| doc.id == *id) {
                    doc.dirty = false;
                }
            }
            self.pending_close = None;
            self.execute_close(ctx, target);
        } else if cancel || modal.should_close() {
            self.pending_close = None;
        }
    }

    /// Fragt nach, wenn eine extern geänderte Datei eigene Änderungen hat.
    fn reload_dialog(&mut self, ctx: &egui::Context) {
        let Some(&id) = self.reload_requests.first() else {
            return;
        };
        let Some(index) = self.docs.iter().position(|doc| doc.id == id) else {
            self.reload_requests.remove(0);
            return;
        };
        let name = self.docs[index].title();

        let mut reload = false;
        let mut keep = false;
        let modal = egui::Modal::new(egui::Id::new("external_change_modal")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("Datei wurde extern geändert");
            ui.add_space(8.0);
            ui.label(format!(
                "Die Datei „{name}“ wurde außerhalb des Editors geändert und enthält ungespeicherte Änderungen."
            ));
            ui.add_space(6.0);
            ui.label("Soll die Datei von der Festplatte neu geladen werden?");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Neu laden").clicked() {
                    reload = true;
                }
                if ui.button("Behalten").clicked() {
                    keep = true;
                }
            });
        });

        if reload {
            self.reload_document(id);
            self.reload_requests.remove(0);
        } else if keep || modal.should_close() {
            self.reload_requests.remove(0);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.applied_theme != Some(self.theme) {
            self.apply_theme(ctx);
            self.applied_theme = Some(self.theme);
        }

        self.handle_incoming_files(ctx);
        self.check_external_changes(ctx);
        self.handle_window_close(ctx);
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);

        self.menu_bar(ctx);
        self.tab_bar(ctx);
        self.status_bar(ctx);
        self.file_panel(ctx);
        self.central_panel(ctx);
        self.dialogs(ctx);
        self.draw_drop_overlay(ctx);

        if self.window_close_allowed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        self.update_title(ctx);

        // Regelmäßig aufwecken, damit externe Dateiänderungen auch im
        // Leerlauf erkannt werden.
        ctx.request_repaint_after(FILE_WATCH_INTERVAL);
    }
}

// ---------------------------------------------------------------------------
// Menü-Helfer
// ---------------------------------------------------------------------------

fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: Option<&str>) -> bool {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(shortcut);
    }
    ui.add(button).clicked()
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
        if let Some(id) = app.active {
            if let Some(index) = app.docs.iter().position(|doc| doc.id == id) {
                let mut doc = app.docs.remove(index);
                if doc.kind == DocKind::Markdown {
                    editor::apply_format(ctx, &mut doc, action, &mut app.status);
                }
                app.docs.insert(index, doc);
            }
        }
        ui.close();
    }
}
