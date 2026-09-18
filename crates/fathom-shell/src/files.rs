//! Export… and Import…, on both targets.
//!
//! Natively they are the OS dialogs. On the web, Import… is the browser's file picker and
//! Export… is a download, because that is what those words mean there. Either way the
//! result arrives later, on a channel the shell polls once a frame: a native dialog
//! runs on its own thread so the window keeps repainting behind it, and a web dialog
//! is a promise.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;

fn dropped_name(file: &egui::DroppedFileHandle) -> String {
    file.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Something a dialog produced.
#[derive(Debug)]
pub enum FileEvent {
    /// A file was chosen and read.
    Opened { name: String, bytes: Vec<u8> },
    /// A file was written. Natively `path` is where; on the web it is the download's
    /// file name.
    Saved { path: PathBuf },
    /// A dialog was dismissed, or a write failed.
    Failed(String),
}

pub struct Dialogs {
    tx: Sender<FileEvent>,
    rx: Receiver<FileEvent>,
}

impl Default for Dialogs {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx }
    }
}

impl Dialogs {
    /// Whatever has arrived since the last poll.
    pub fn poll(&self) -> Vec<FileEvent> {
        self.rx.try_iter().collect()
    }

    /// Show the Import… dialog. `directory` is where it starts, when the target has such a
    /// thing; `extensions` filters what it shows, without the dot.
    pub fn open(&self, directory: Option<PathBuf>, extensions: &'static [&'static str]) {
        platform::open(self.tx.clone(), directory, extensions);
    }

    /// Read a file dropped on the window and deliver it as [`FileEvent::Opened`].
    pub fn read_dropped(&self, file: egui::DroppedFileHandle) {
        platform::read_dropped(self.tx.clone(), file);
    }

    /// Show the Export… dialog for `bytes`, suggesting `file_name`.
    pub fn save(&self, directory: Option<PathBuf>, file_name: String, extensions: &'static [&'static str], bytes: Vec<u8>) {
        platform::save(self.tx.clone(), directory, file_name, extensions, bytes);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::*;

    fn dialog(directory: Option<PathBuf>, extensions: &'static [&'static str]) -> rfd::FileDialog {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = directory.and_then(|d| std::fs::canonicalize(d).ok()) {
            dialog = dialog.set_directory(dir);
        }
        if !extensions.is_empty() {
            dialog = dialog.add_filter(extensions.join(", "), extensions);
        }
        dialog
    }

    pub fn open(tx: Sender<FileEvent>, directory: Option<PathBuf>, extensions: &'static [&'static str]) {
        std::thread::spawn(move || {
            let event = match dialog(directory, extensions).pick_file() {
                Some(path) => match std::fs::read(&path) {
                    Ok(bytes) => FileEvent::Opened {
                        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        bytes,
                    },
                    Err(e) => FileEvent::Failed(format!("could not read {}: {e}", path.display())),
                },
                None => FileEvent::Failed("open cancelled".into()),
            };
            let _ = tx.send(event);
        });
    }

    pub fn read_dropped(tx: Sender<FileEvent>, file: egui::DroppedFileHandle) {
        let name = dropped_name(&file);
        let event = match file.bytes() {
            Ok(bytes) => FileEvent::Opened { name, bytes },
            Err(e) => FileEvent::Failed(format!("could not read {name}: {e}")),
        };
        let _ = tx.send(event);
    }

    pub fn save(
        tx: Sender<FileEvent>,
        directory: Option<PathBuf>,
        file_name: String,
        extensions: &'static [&'static str],
        bytes: Vec<u8>,
    ) {
        std::thread::spawn(move || {
            let event = match dialog(directory, extensions).set_file_name(file_name).save_file() {
                Some(path) => match std::fs::write(&path, &bytes) {
                    Ok(()) => FileEvent::Saved { path },
                    Err(e) => FileEvent::Failed(format!("could not write {}: {e}", path.display())),
                },
                None => FileEvent::Failed("save cancelled".into()),
            };
            let _ = tx.send(event);
        });
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::*;
    use eframe::wasm_bindgen::JsCast;

    pub fn open(tx: Sender<FileEvent>, _directory: Option<PathBuf>, extensions: &'static [&'static str]) {
        let mut dialog = rfd::AsyncFileDialog::new();
        if !extensions.is_empty() {
            dialog = dialog.add_filter(extensions.join(", "), extensions);
        }
        wasm_bindgen_futures::spawn_local(async move {
            let event = match dialog.pick_file().await {
                Some(handle) => FileEvent::Opened { name: handle.file_name(), bytes: handle.read().await },
                None => FileEvent::Failed("open cancelled".into()),
            };
            let _ = tx.send(event);
        });
    }

    pub fn read_dropped(tx: Sender<FileEvent>, file: egui::DroppedFileHandle) {
        let name = dropped_name(&file);
        wasm_bindgen_futures::spawn_local(async move {
            let event = match file.bytes_async().await {
                Ok(bytes) => FileEvent::Opened { name, bytes },
                Err(e) => FileEvent::Failed(format!("could not read {name}: {e}")),
            };
            let _ = tx.send(event);
        });
    }

    /// A download: a blob URL on an anchor with the `download` attribute, clicked. The
    /// browser decides where it goes, as it does for every download.
    pub fn save(
        tx: Sender<FileEvent>,
        _directory: Option<PathBuf>,
        file_name: String,
        _extensions: &'static [&'static str],
        bytes: Vec<u8>,
    ) {
        let event = match download(&file_name, &bytes) {
            Ok(()) => FileEvent::Saved { path: PathBuf::from(file_name) },
            Err(e) => FileEvent::Failed(format!("download failed: {e:?}")),
        };
        let _ = tx.send(event);
    }

    fn download(file_name: &str, bytes: &[u8]) -> Result<(), eframe::wasm_bindgen::JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let array = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::new();
        parts.push(&array.buffer());
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("application/json");
        let blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &options)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        let anchor: web_sys::HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
        anchor.set_href(&url);
        anchor.set_download(file_name);
        anchor.click();
        web_sys::Url::revoke_object_url(&url)?;
        Ok(())
    }
}
