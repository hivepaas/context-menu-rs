//! The plan written to the registry, and what is there read back.

use std::path::PathBuf;

use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use super::{Key, MARK, data_home, definition_path, plan};
use crate::{Error, Menu, Report};

/// Where the menu goes: the classes key under `HKEY_CURRENT_USER`, and the
/// data folder its definition goes in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// `Software\Classes` for the user's own, which Explorer reads.
    pub classes: String,
    /// `%LOCALAPPDATA%`.
    pub data_home: PathBuf,
}

impl Root {
    /// The current user's.
    pub fn current() -> Result<Root, Error> {
        Ok(Root {
            classes: r"Software\Classes".into(),
            data_home: data_home()?,
        })
    }

    /// The full name of `path`, for a report.
    fn name(&self, path: &str) -> PathBuf {
        PathBuf::from(format!(r"HKEY_CURRENT_USER\{}\{path}", self.classes))
    }

    fn error(&self, path: &str, source: std::io::Error) -> Error {
        Error::Io {
            path: self.name(path),
            source,
        }
    }

    /// The classes key, made if it is not there.
    fn open(&self) -> Result<RegKey, Error> {
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&self.classes)
            .map(|(k, _)| k)
            .map_err(|e| self.error("", e))
    }
}

/// The menu's verbs there are: keys under a class's `shell` key, marked
/// with its id.
fn existing(classes: &RegKey, id: &str) -> Vec<String> {
    let mut shells = vec![r"*\shell".to_string(), r"Directory\shell".to_string()];
    if let Ok(associations) = classes.open_subkey("SystemFileAssociations") {
        for name in associations.enum_keys().flatten() {
            shells.push(format!(r"SystemFileAssociations\{name}\shell"));
        }
    }
    let mut found = Vec::new();
    for shell in shells {
        let Ok(key) = classes.open_subkey(&shell) else {
            continue;
        };
        for verb in key.enum_keys().flatten() {
            let marked = key
                .open_subkey(&verb)
                .and_then(|v| v.get_value::<String, _>(MARK))
                .is_ok_and(|v| v == id);
            if marked {
                found.push(format!(r"{shell}\{verb}"));
            }
        }
    }
    found
}

/// The keys under `path`, it first, as the plan has them.
fn read(classes: &RegKey, path: &str, out: &mut Vec<Key>) -> std::io::Result<()> {
    let key = classes.open_subkey(path)?;
    let mut values = Vec::new();
    for (name, _) in key.enum_values().flatten() {
        // A value that is not a string is not one the plan writes.
        let value = key
            .get_value::<String, _>(&name)
            .unwrap_or_else(|_| "\u{fffd}".into());
        values.push((name, value));
    }
    out.push(Key {
        path: path.into(),
        values,
    });
    for sub in key.enum_keys().flatten() {
        read(classes, &format!(r"{path}\{sub}"), out)?;
    }
    Ok(())
}

/// The keys of a plan, or of what is there, in one order to compare them.
fn sorted(mut keys: Vec<Key>) -> Vec<Key> {
    for k in &mut keys {
        k.values.sort();
    }
    keys.sort();
    keys
}

/// The verbs among `keys`: those marked with the menu's id.
fn verbs(keys: &[Key]) -> Vec<&str> {
    keys.iter()
        .filter(|k| k.values.iter().any(|(n, _)| n == MARK))
        .map(|k| k.path.as_str())
        .collect()
}

/// Installs `menu` under `root`, in place of an earlier install.
pub fn install(menu: &Menu, root: &Root) -> Result<Report, Error> {
    menu.validate()?;
    let classes = root.open()?;
    let keys = plan(menu);
    let wanted = verbs(&keys);
    let mut report = Report::default();
    for path in existing(&classes, &menu.id) {
        classes
            .delete_subkey_all(&path)
            .map_err(|e| root.error(&path, e))?;
        if !wanted.contains(&path.as_str()) {
            report.removed.push(root.name(&path));
        }
    }
    for key in &keys {
        let (k, _) = classes
            .create_subkey(&key.path)
            .map_err(|e| root.error(&key.path, e))?;
        for (name, value) in &key.values {
            k.set_value(name, value)
                .map_err(|e| root.error(&key.path, e))?;
        }
    }
    report.written = wanted.iter().map(|p| root.name(p)).collect();

    let definition = definition_path(&menu.id, &root.data_home);
    let dir = definition.parent().unwrap_or(&root.data_home);
    std::fs::create_dir_all(dir).map_err(|source| Error::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    std::fs::write(&definition, menu.to_definition()).map_err(|source| Error::Io {
        path: definition.clone(),
        source,
    })?;
    report.written.push(definition);
    report.notes.push(
        "Windows: the items are in the classic menu, which Windows 11 shows under \
         \"Show more options\"; Windows runs the program once for each file chosen."
            .into(),
    );
    Ok(report)
}

/// Takes the menu `id` away from under `root`.
pub fn uninstall(id: &str, root: &Root) -> Result<Report, Error> {
    let mut report = Report::default();
    if let Ok(classes) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&root.classes) {
        for path in existing(&classes, id) {
            classes
                .delete_subkey_all(&path)
                .map_err(|e| root.error(&path, e))?;
            report.removed.push(root.name(&path));
        }
    }
    let definition = definition_path(id, &root.data_home);
    match std::fs::remove_file(&definition) {
        Ok(()) => report.removed.push(definition),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                path: definition,
                source,
            });
        }
    }
    Ok(report)
}

/// Whether the registry under `root`, and the definition, hold `menu` as
/// [`install`] writes it, and nothing else of the menu's.
pub fn is_current(menu: &Menu, root: &Root) -> Result<bool, Error> {
    menu.validate()?;
    let Ok(classes) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&root.classes) else {
        return Ok(false);
    };
    let mut there = Vec::new();
    for path in existing(&classes, &menu.id) {
        read(&classes, &path, &mut there).map_err(|e| root.error(&path, e))?;
    }
    let definition = std::fs::read_to_string(definition_path(&menu.id, &root.data_home));
    Ok(sorted(there) == sorted(plan(menu))
        && definition.is_ok_and(|text| text == menu.to_definition()))
}
