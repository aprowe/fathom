//! Saved parameter sets, as files, from two places.
//!
//! A preset is one JSON document:
//!
//! ```json
//! { "app": "Gravity", "params": { "g": 1.0, "trails": 1 }, "state": null }
//! ```
//!
//! `params` is the parameter block by name — see [`ParamBlock::to_json`] — and `state`
//! is whatever the app's [`save_state`](crate::ShellApp::save_state) returned. A file
//! that is only the `params` object, which is what earlier versions wrote, still loads.
//!
//! Presets come from two sources, merged into one list:
//!
//! * **built in** — JSON files under the app crate's `presets/` folder, compiled into the
//!   binary with [`builtin_presets!`](fathom_core::builtin_presets), so the web build
//!   ships with some;
//! * **the folder** — `presets/<app>/` under the directory the app was launched from,
//!   native only. Next to the project rather than in a hidden config directory, because
//!   a preset is part of the work: it gets committed, diffed and sent to someone.
//!
//! A folder file shadows a built-in one of the same name. Every function takes the
//! folder's root so the tests can point it at a scratch directory; the shell passes
//! [`default_root`].

use std::io;
use std::path::{Path, PathBuf};

use fathom_core::include_dir::Dir;
use fathom_core::{ParamBlock, ParamDef};
use serde_json::Value;

/// Where a listed preset came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Compiled into the binary. Cannot be deleted.
    BuiltIn,
    /// A file under `presets/<app>/`.
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    pub source: Source,
}

/// A preset file, decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// The app that wrote it, or empty for a params-only file from before the field
    /// existed.
    pub app: String,
    pub params: Value,
    pub state: Value,
}

impl Document {
    /// Whether this file belongs to `app`. A file with no `app` field is given the
    /// benefit of the doubt.
    pub fn is_for(&self, app: &str) -> bool {
        self.app.is_empty() || self.app == app
    }

    /// Apply the parameters to the block. Returns how many it set.
    pub fn apply(&self, schema: &[ParamDef], block: &mut ParamBlock) -> usize {
        block.apply_json(schema, &self.params)
    }
}

/// The document for the current block and state, as pretty JSON.
pub fn encode(app: &str, schema: &[ParamDef], block: &ParamBlock, state: Value) -> String {
    let doc = serde_json::json!({ "app": app, "params": block.to_json(schema), "state": state });
    serde_json::to_string_pretty(&doc).expect("a Value always serialises")
}

/// A document from its text. Accepts the current shape and the older params-only one.
pub fn decode(text: &str) -> io::Result<Document> {
    let value: Value = serde_json::from_str(text)?;
    let Some(object) = value.as_object() else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "a preset is a JSON object"));
    };
    Ok(match object.get("params") {
        Some(params) => Document {
            app: object.get("app").and_then(Value::as_str).unwrap_or("").to_string(),
            params: params.clone(),
            state: object.get("state").cloned().unwrap_or(Value::Null),
        },
        None => Document { app: String::new(), params: value.clone(), state: Value::Null },
    })
}

/// `presets/` under the current directory.
pub fn default_root() -> PathBuf {
    PathBuf::from("presets")
}

/// A file name from whatever was typed: trimmed, path separators and control characters
/// replaced, so a name can never reach outside the app's folder.
pub fn sanitise(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect::<String>()
        .trim_matches('.')
        .to_string()
}

#[cfg(not(target_arch = "wasm32"))]
fn path_for(root: &Path, app: &str, name: &str) -> PathBuf {
    root.join(sanitise(app)).join(format!("{}.json", sanitise(name)))
}

/// Every preset available: built in, then the folder's, merged by name and sorted.
pub fn list(builtin: Option<&Dir<'static>>, root: &Path, app: &str) -> Vec<Preset> {
    let mut presets: Vec<Preset> = builtin
        .map(builtin_names)
        .unwrap_or_default()
        .into_iter()
        .map(|name| Preset { name, source: Source::BuiltIn })
        .collect();
    for name in folder_names(root, app) {
        match presets.iter_mut().find(|p| p.name == name) {
            Some(p) => p.source = Source::Folder,
            None => presets.push(Preset { name, source: Source::Folder }),
        }
    }
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    presets
}

fn builtin_names(dir: &Dir<'static>) -> Vec<String> {
    dir.files()
        .filter(|f| f.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|f| f.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect()
}

fn builtin_text(dir: &Dir<'static>, name: &str) -> Option<&'static str> {
    dir.get_file(format!("{name}.json")).and_then(|f| f.contents_utf8())
}

/// The text of a preset by name: the folder's copy if there is one, else the built-in.
pub fn read(builtin: Option<&Dir<'static>>, root: &Path, app: &str, name: &str) -> io::Result<String> {
    match folder_read(root, app, name) {
        Ok(text) => Ok(text),
        Err(folder_err) => builtin
            .and_then(|d| builtin_text(d, name))
            .map(str::to_string)
            .ok_or(folder_err),
    }
}

/// Write `text` as `<root>/<app>/<name>.json`, creating the folders. Returns the path.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(root: &Path, app: &str, name: &str, text: &str) -> io::Result<PathBuf> {
    let name = sanitise(name);
    if name.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a preset needs a name"));
    }
    let path = path_for(root, app, &name);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn delete(root: &Path, app: &str, name: &str) -> io::Result<()> {
    std::fs::remove_file(path_for(root, app, name))
}

#[cfg(not(target_arch = "wasm32"))]
fn folder_read(root: &Path, app: &str, name: &str) -> io::Result<String> {
    std::fs::read_to_string(path_for(root, app, name))
}

#[cfg(not(target_arch = "wasm32"))]
fn folder_names(root: &Path, app: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(sanitise(app))) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if path.extension().is_some_and(|x| x == "json") {
                path.file_stem().map(|s| s.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect()
}

// The web has no folder. The built-in list is the whole list, and Save… downloads.
#[cfg(target_arch = "wasm32")]
fn folder_read(_root: &Path, _app: &str, name: &str) -> io::Result<String> {
    Err(io::Error::new(io::ErrorKind::NotFound, format!("no preset named {name}")))
}

#[cfg(target_arch = "wasm32")]
fn folder_names(_root: &Path, _app: &str) -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fathom_core::include_dir::{Dir, File};
    use std::fs;

    const SCHEMA: &[ParamDef] = &[
        ParamDef::float("g", "Gravity", 1.5, 0.0, 5.0),
        ParamDef::toggle("trails", "Trails", true),
        ParamDef::choice("mode", "Mode", 2, &["a", "b", "c"]),
    ];

    static BUILTIN: Dir<'static> = Dir::new(
        "presets",
        &[
            fathom_core::include_dir::DirEntry::File(File::new(
                "calm.json",
                br#"{ "app": "app", "params": { "g": 0.5 }, "state": { "seed": 7 } }"#,
            )),
            fathom_core::include_dir::DirEntry::File(File::new("shared.json", br#"{ "g": 1.0 }"#)),
            fathom_core::include_dir::DirEntry::File(File::new("notes.txt", b"not a preset")),
        ],
    );

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fathom-presets-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_document_round_trips_params_and_state() {
        let mut block = ParamBlock::from_defaults(SCHEMA);
        block.set_f32(0, 4.0);
        block.set_u32(2, 0);
        let text = encode("Demo", SCHEMA, &block, serde_json::json!({ "bodies": [1, 2] }));
        let doc = decode(&text).unwrap();
        assert_eq!(doc.app, "Demo");
        assert_eq!(doc.state["bodies"][1], 2);
        assert!(doc.is_for("Demo"));
        assert!(!doc.is_for("Other"));

        let mut fresh = ParamBlock::from_defaults(SCHEMA);
        assert_eq!(doc.apply(SCHEMA, &mut fresh), 3);
        assert_eq!(fresh.f32(0), 4.0);
        assert_eq!(fresh.u32(2), 0);
    }

    /// The files written before `app` and `state` existed were the params object alone.
    #[test]
    fn a_params_only_file_still_loads() {
        let doc = decode(r#"{ "g": 2.5, "trails": 0 }"#).unwrap();
        assert_eq!(doc.app, "");
        assert!(doc.is_for("anything"));
        assert_eq!(doc.state, Value::Null);
        let mut block = ParamBlock::from_defaults(SCHEMA);
        assert_eq!(doc.apply(SCHEMA, &mut block), 2);
        assert_eq!(block.f32(0), 2.5);
        assert!(decode("[1, 2]").is_err());
        assert!(decode("nonsense").is_err());
    }

    #[test]
    fn built_in_and_folder_presets_merge_and_the_folder_wins() {
        let root = scratch("merge");
        save(&root, "app", "shared", r#"{ "g": 3.0 }"#).unwrap();
        save(&root, "app", "mine", r#"{ "g": 2.0 }"#).unwrap();

        let listed = list(Some(&BUILTIN), &root, "app");
        let names: Vec<(&str, Source)> = listed.iter().map(|p| (p.name.as_str(), p.source)).collect();
        assert_eq!(
            names,
            vec![("calm", Source::BuiltIn), ("mine", Source::Folder), ("shared", Source::Folder)],
            "sorted, merged by name, and notes.txt ignored"
        );

        let shared = decode(&read(Some(&BUILTIN), &root, "app", "shared").unwrap()).unwrap();
        assert_eq!(shared.params["g"], 3.0, "the folder's copy shadows the built-in one");
        let calm = decode(&read(Some(&BUILTIN), &root, "app", "calm").unwrap()).unwrap();
        assert_eq!(calm.state["seed"], 7);
        assert!(read(Some(&BUILTIN), &root, "app", "missing").is_err());

        delete(&root, "app", "mine").unwrap();
        assert_eq!(list(Some(&BUILTIN), &root, "app").len(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    /// The name is typed by a person, so it is data, not a path.
    #[test]
    fn a_name_cannot_escape_the_apps_folder() {
        assert_eq!(sanitise("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitise("  fast chasers  "), "fast chasers");
        assert_eq!(sanitise("a:b"), "a_b");
        let root = scratch("escape");
        assert!(save(&root, "app", "   ", "{}").is_err(), "an empty name saves nothing");
        let path = save(&root, "app", "..\\up", "{}").unwrap();
        assert!(path.starts_with(root.join("app")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_app_with_nothing_saved_lists_nothing() {
        assert!(list(None, &scratch("empty"), "nothing").is_empty());
    }
}
