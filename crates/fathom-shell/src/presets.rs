//! Saved parameter sets, as files.
//!
//! A preset is the whole parameter block written by name — see
//! [`ParamBlock::to_json`] — to `presets/<app>/<name>.json` under the directory the app
//! was launched from. Next to the project rather than in a hidden config directory,
//! because a preset is part of the work: it gets committed, diffed and sent to someone.
//!
//! Every function takes the store's root so the tests can point it at a scratch
//! directory; the shell passes [`default_root`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use fathom_core::{ParamBlock, ParamDef};

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

fn path_for(root: &Path, app: &str, name: &str) -> PathBuf {
    root.join(sanitise(app)).join(format!("{}.json", sanitise(name)))
}

/// Write the block as `<root>/<app>/<name>.json`, creating the folders. Returns the path.
pub fn save(root: &Path, app: &str, name: &str, schema: &[ParamDef], block: &ParamBlock) -> io::Result<PathBuf> {
    let name = sanitise(name);
    if name.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a preset needs a name"));
    }
    let path = path_for(root, app, &name);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&block.to_json(schema))?;
    fs::write(&path, text)?;
    Ok(path)
}

/// Read `<root>/<app>/<name>.json` into the block. Returns how many parameters it set.
pub fn load(root: &Path, app: &str, name: &str, schema: &[ParamDef], block: &mut ParamBlock) -> io::Result<usize> {
    let text = fs::read_to_string(path_for(root, app, name))?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    Ok(block.apply_json(schema, &value))
}

pub fn delete(root: &Path, app: &str, name: &str) -> io::Result<()> {
    fs::remove_file(path_for(root, app, name))
}

/// The names saved for an app, sorted. An app with no folder yet has none.
pub fn list(root: &Path, app: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root.join(sanitise(app))) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if path.extension().is_some_and(|x| x == "json") {
                path.file_stem().map(|s| s.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &[ParamDef] = &[
        ParamDef::float("g", "Gravity", 1.5, 0.0, 5.0),
        ParamDef::toggle("trails", "Trails", true),
        ParamDef::choice("mode", "Mode", 2, &["a", "b", "c"]),
    ];

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fathom-presets-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_preset_saves_lists_loads_and_deletes() {
        let root = scratch("cycle");
        let mut block = ParamBlock::from_defaults(SCHEMA);
        block.set_f32(0, 4.0);
        block.set_u32(2, 0);
        let path = save(&root, "Demo App", "my run", SCHEMA, &block).unwrap();
        assert!(path.ends_with(Path::new("Demo App").join("my run.json")));
        assert_eq!(list(&root, "Demo App"), vec!["my run".to_string()]);

        let mut fresh = ParamBlock::from_defaults(SCHEMA);
        assert_eq!(load(&root, "Demo App", "my run", SCHEMA, &mut fresh).unwrap(), 3);
        assert_eq!(fresh.f32(0), 4.0);
        assert_eq!(fresh.u32(2), 0);

        delete(&root, "Demo App", "my run").unwrap();
        assert!(list(&root, "Demo App").is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    /// The name is typed by a person, so it is data, not a path.
    #[test]
    fn a_name_cannot_escape_the_apps_folder() {
        assert_eq!(sanitise("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitise("  fast chasers  "), "fast chasers");
        assert_eq!(sanitise("a:b"), "a_b");
        let root = scratch("escape");
        let block = ParamBlock::from_defaults(SCHEMA);
        assert!(save(&root, "app", "   ", SCHEMA, &block).is_err(), "an empty name saves nothing");
        let path = save(&root, "app", "..\\up", SCHEMA, &block).unwrap();
        assert!(path.starts_with(root.join("app")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_app_with_nothing_saved_lists_nothing() {
        assert!(list(&scratch("empty"), "nothing").is_empty());
    }
}
