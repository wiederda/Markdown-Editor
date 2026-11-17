//! Betriebssystem-Integration: Dateien im Explorer anzeigen und die Anwendung
//! unter „Öffnen mit“ registrieren.

use std::path::Path;
use std::process::Command;

/// Zeigt den Ordner einer Datei im Dateimanager an und markiert die Datei.
pub fn reveal(path: &Path) -> bool {
    if cfg!(target_os = "windows") {
        let path = path.to_string_lossy().replace('/', "\\");
        Command::new("explorer")
            .arg(format!("/select,\"{path}\""))
            .spawn()
            .is_ok()
    } else {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .is_ok()
    }
}

/// Registriert die Anwendung so, dass sie im Kontextmenü „Öffnen mit“ für
/// Markdown- und HTML-Dateien auftaucht.
pub fn register_open_with(exe: &Path) -> bool {
    if cfg!(target_os = "windows") {
        register_windows(exe)
    } else {
        register_linux(exe)
    }
}

fn register_windows(exe: &Path) -> bool {
    let exe_string = exe.to_string_lossy().into_owned();
    let prog_id = "MDEditorFile";
    let command = format!("\"{exe_string}\" \"%1\"");
    let icon = format!("{exe_string},0");

    let steps = [
        (
            format!("HKCU\\Software\\Classes\\{prog_id}\\shell\\open\\command"),
            command.as_str(),
        ),
        (
            format!("HKCU\\Software\\Classes\\{prog_id}\\DefaultIcon"),
            icon.as_str(),
        ),
    ];

    for (key, value) in steps {
        let mut reg = Command::new("reg");
        reg.arg("add")
            .arg(&key)
            .arg("/ve")
            .arg("/d")
            .arg(value)
            .arg("/f");
        if reg.status().map(|s| !s.success()).unwrap_or(true) {
            return false;
        }
    }

    for extension in [".md", ".markdown", ".mdown", ".mkd", ".html", ".htm"] {
        let key = format!("HKCU\\Software\\Classes\\{extension}\\OpenWithProgids");
        let mut reg = Command::new("reg");
        reg.arg("add")
            .arg(&key)
            .arg("/v")
            .arg(prog_id)
            .arg("/f");
        if reg.status().map(|s| !s.success()).unwrap_or(true) {
            return false;
        }
    }
    true
}

fn register_linux(exe: &Path) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let exe_string = exe.to_string_lossy().into_owned();
    let dir = std::path::PathBuf::from(home).join(".local/share/applications");
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let desktop = dir.join("md-editor.desktop");
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=MD Editor\n\
         Exec={exe_string} %F\n\
         MimeType=text/markdown;text/html;\n\
         Terminal=false\n"
    );
    std::fs::write(desktop, content).is_ok()
}
