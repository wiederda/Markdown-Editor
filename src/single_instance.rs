//! Einzelinstanz-Betrieb: Läuft bereits ein Editor, werden per „Öffnen mit“
//! übergebene Dateien an die laufende Instanz weitergereicht und dort in einem
//! neuen Tab geöffnet, statt ein zweites Fenster zu starten.
//!
//! Die Kommunikation läuft über einen Loopback-Port: Die erste Instanz bindet
//! ihn und nimmt Anfragen entgegen, weitere Instanzen verbinden sich, schicken
//! ihre Dateipfade und beenden sich danach sofort.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui;

/// Nur lokal erreichbarer Port, über den die Instanzen miteinander sprechen.
const ADDRESS: &str = "127.0.0.1:47821";
const GREETING: &str = "MD-EDITOR/1";
const RESPONSE_OK: &str = "OK";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_millis(700);

/// Ergebnis des Startvorgangs.
pub enum Startup {
    /// Diese Instanz ist die erste und nimmt Dateien anderer Instanzen an.
    Primary(Primary),
    /// Eine andere Instanz läuft bereits und hat die Anfrage übernommen.
    Forwarded,
    /// Unklar, ob eine andere Instanz läuft – dann normal starten.
    Standalone,
}

pub struct Primary {
    listener: Option<TcpListener>,
    sender: Sender<PathBuf>,
    receiver: Receiver<PathBuf>,
}

impl Primary {
    /// Startet den Empfänger-Thread und gibt den Kanal für die Oberfläche zurück.
    pub fn attach(self, ctx: egui::Context) -> Receiver<PathBuf> {
        let Primary {
            listener,
            sender,
            receiver,
        } = self;
        if let Some(listener) = listener {
            let _ = std::thread::Builder::new()
                .name("md-editor-single-instance".to_owned())
                .spawn(move || serve(listener, sender, ctx));
        }
        receiver
    }
}

/// Prüft, ob eine Instanz läuft. Ist das der Fall, werden die Pfade übergeben.
pub fn startup(args: &[PathBuf]) -> Startup {
    if forward(args) {
        return Startup::Forwarded;
    }

    match TcpListener::bind(ADDRESS) {
        Ok(listener) => {
            let (sender, receiver) = mpsc::channel();
            Startup::Primary(Primary {
                listener: Some(listener),
                sender,
                receiver,
            })
        }
        Err(_) => {
            // Eine parallel startende Instanz belegt den Port evtl. gerade erst.
            for _ in 0..12 {
                std::thread::sleep(Duration::from_millis(40));
                if forward(args) {
                    return Startup::Forwarded;
                }
            }
            Startup::Standalone
        }
    }
}

fn forward(args: &[PathBuf]) -> bool {
    let Ok(address) = ADDRESS.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) else {
        return false;
    };
    if stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
        || stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return false;
    }

    let mut payload = String::new();
    payload.push_str(GREETING);
    payload.push('\n');
    for path in args {
        let text = path.to_string_lossy().replace(['\r', '\n'], " ");
        if text.is_empty() {
            continue;
        }
        payload.push_str(&text);
        payload.push('\n');
    }
    payload.push('\n');

    if stream.write_all(payload.as_bytes()).is_err() || stream.flush().is_err() {
        return false;
    }

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    if reader.read_line(&mut response).is_err() {
        return false;
    }
    response.trim_end() == RESPONSE_OK
}

fn serve(listener: TcpListener, sender: Sender<PathBuf>, ctx: egui::Context) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let Some(paths) = read_request(&mut stream) else {
            continue;
        };
        let _ = stream.write_all(format!("{RESPONSE_OK}\n").as_bytes());
        let _ = stream.flush();
        for path in paths {
            if sender.send(path).is_err() {
                return;
            }
        }
        // Weckt die Oberfläche, damit ankommende Dateien sofort geöffnet werden.
        ctx.request_repaint();
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Vec<PathBuf>> {
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;

    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 || line.trim_end() != GREETING {
        return None;
    }

    let mut paths = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
        if trimmed.is_empty() {
            break;
        }
        paths.push(PathBuf::from(trimmed));
    }
    Some(paths)
}
