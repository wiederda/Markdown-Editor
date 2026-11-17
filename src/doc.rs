use std::path::{Path, PathBuf};

use eframe::egui;

/// Unterstützte Markdown-Dateiendungen.
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd"];
/// Unterstützte HTML-Dateiendungen.
pub const HTML_EXTENSIONS: &[&str] = &["html", "htm"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind {
    Markdown,
    Html,
}

impl DocKind {
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("html") | Some("htm") => Self::Html,
            _ => Self::Markdown,
        }
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.to_ascii_lowercase();
        if MARKDOWN_EXTENSIONS.contains(&extension.as_str()) {
            Some(Self::Markdown)
        } else if HTML_EXTENSIONS.contains(&extension.as_str()) {
            Some(Self::Html)
        } else {
            None
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Markdown => MARKDOWN_EXTENSIONS,
            Self::Html => HTML_EXTENSIONS,
        }
    }

    pub fn filter_name(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Html => "HTML",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Markdown => "MD",
            Self::Html => "HTML",
        }
    }

    pub fn editor_title(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown-Editor",
            Self::Html => "HTML-Editor",
        }
    }

    pub fn default_file_name(self) -> &'static str {
        match self {
            Self::Markdown => "Unbenannt.md",
            Self::Html => "Unbenannt.html",
        }
    }
}

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(DocKind::from_extension)
        .is_some()
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollMaster {
    #[default]
    Editor,
    Preview,
}

/// Zustand einer Datei auf der Festplatte (Änderungszeitpunkt und Größe),
/// mit dem externe Änderungen an geöffneten Dateien erkannt werden.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct FileStamp {
    modified: Option<std::time::SystemTime>,
    len: u64,
}

impl FileStamp {
    pub fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// Ein geöffnetes Dokument in einem Tab.
pub struct Document {
    pub id: u64,
    pub kind: DocKind,
    pub path: Option<PathBuf>,
    pub text: String,
    pub dirty: bool,
    pub untitled_name: Option<String>,
    /// Zuletzt gesehener Dateizustand – für die Erkennung externer Änderungen.
    pub stamp: Option<FileStamp>,

    // Suchen & Ersetzen
    pub find_open: bool,
    pub find: String,
    pub replace: String,
    pub match_case: bool,

    // Cursorposition (für die Statusleiste)
    pub cursor_line: usize,
    pub cursor_column: usize,

    // Synchronisiertes Scrollen
    pub editor_offset: f32,
    pub editor_max: f32,
    pub preview_offset: f32,
    pub preview_max: f32,
    pub scroll_master: ScrollMaster,
    pub sync_ratio: f32,
}

impl Document {
    pub fn untitled(id: u64, kind: DocKind, name: String) -> Self {
        Self {
            id,
            kind,
            path: None,
            text: String::new(),
            dirty: false,
            untitled_name: Some(name),
            stamp: None,
            find_open: false,
            find: String::new(),
            replace: String::new(),
            match_case: false,
            cursor_line: 1,
            cursor_column: 1,
            editor_offset: 0.0,
            editor_max: 1.0,
            preview_offset: 0.0,
            preview_max: 1.0,
            scroll_master: ScrollMaster::Editor,
            sync_ratio: 0.0,
        }
    }

    pub fn from_path(id: u64, path: PathBuf, text: String) -> Self {
        let kind = DocKind::from_path(&path);
        let stamp = FileStamp::read(&path);
        Self {
            id,
            kind,
            path: Some(path),
            text,
            dirty: false,
            untitled_name: None,
            stamp,
            find_open: false,
            find: String::new(),
            replace: String::new(),
            match_case: false,
            cursor_line: 1,
            cursor_column: 1,
            editor_offset: 0.0,
            editor_max: 1.0,
            preview_offset: 0.0,
            preview_max: 1.0,
            scroll_master: ScrollMaster::Editor,
            sync_ratio: 0.0,
        }
    }

    pub fn file_name(&self) -> Option<String> {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    pub fn title(&self) -> String {
        self.file_name().unwrap_or_else(|| {
            self.untitled_name
                .clone()
                .unwrap_or_else(|| "Unbenannt".to_owned())
        })
    }

    pub fn base_dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    pub fn editor_id(&self) -> egui::Id {
        egui::Id::new(("editor", self.id))
    }

    pub fn reset_view_state(&mut self) {
        self.cursor_line = 1;
        self.cursor_column = 1;
        self.editor_offset = 0.0;
        self.preview_offset = 0.0;
        self.scroll_master = ScrollMaster::Editor;
        self.sync_ratio = 0.0;
    }
}

/// Ermittelt einen relativen Pfad von `from_dir` nach `to`.
/// Liefert `None`, wenn es keinen gemeinsamen Ausgangspunkt gibt.
pub fn relative_path(from_dir: &Path, to: &Path) -> Option<PathBuf> {
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

pub fn display_path(path: &Path) -> String {
    path.display().to_string()
}

/// Dateien, die für die Dateiliste gesammelt werden.
pub fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else if is_supported(&path) {
            out.push(path);
        }
    }
}
