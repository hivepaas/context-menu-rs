//! The plan written to the registry, and what is there read back.

use std::path::PathBuf;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

use super::{Key, MARK, clsid, data_home, definition_path, plan};
use crate::{Error, Menu, Report};

/// Where the menu goes: the classes key under `HKEY_CURRENT_USER`, and the
/// data folder its definition goes in; and where Windows says whether an
/// app's package serves the menu with the native extension. A handler goes
/// in the classes key too, and in `RegisteredApplications` beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// `Software\Classes` for the user's own, which Explorer reads.
    pub classes: String,
    /// `%LOCALAPPDATA%`.
    pub data_home: PathBuf,
    /// The packages declaring each COM class, by class, with its hive:
    /// `HKEY_LOCAL_MACHINE\SOFTWARE\Classes\PackagedCom\ClassIndex`.
    pub class_index: String,
    /// The packages registered for the user, with its hive:
    /// `HKEY_CURRENT_USER\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages`.
    pub user_packages: String,
}

impl Root {
    /// The current user's.
    pub fn current() -> Result<Root, Error> {
        Ok(Root {
            classes: r"Software\Classes".into(),
            data_home: data_home()?,
            class_index: r"HKEY_LOCAL_MACHINE\SOFTWARE\Classes\PackagedCom\ClassIndex".into(),
            user_packages: r"HKEY_CURRENT_USER\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages".into(),
        })
    }

    /// The full name of `path`, for a report.
    pub(super) fn name(&self, path: &str) -> PathBuf {
        PathBuf::from(format!(r"HKEY_CURRENT_USER\{}\{path}", self.classes))
    }

    pub(super) fn error(&self, path: &str, source: std::io::Error) -> Error {
        Error::Io {
            path: self.name(path),
            source,
        }
    }

    /// `RegisteredApplications`, beside the classes key: for
    /// `Software\Classes`, `Software\RegisteredApplications`.
    pub(super) fn registered(&self) -> String {
        match self.classes.rsplit_once('\\') {
            Some((parent, _)) => format!(r"{parent}\RegisteredApplications"),
            None => "RegisteredApplications".into(),
        }
    }

    /// The classes key, made if it is not there.
    pub(super) fn open(&self) -> Result<RegKey, Error> {
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&self.classes)
            .map(|(k, _)| k)
            .map_err(|e| self.error("", e))
    }
}

/// The key at `path`, a full name with its hive.
fn open_full(path: &str) -> Option<RegKey> {
    let (hive, rest) = path.split_once('\\')?;
    let hive = match hive {
        "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
        "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
        _ => return None,
    };
    RegKey::predef(hive).open_subkey(rest).ok()
}

/// The package that serves the menu `id` with the native extension, when
/// one declaring its class is registered for the user.
fn package(id: &str, root: &Root) -> Option<String> {
    let declaring = open_full(&format!(r"{}\{{{}}}", root.class_index, clsid(id)))?;
    let registered = open_full(&root.user_packages)?;
    declaring
        .enum_keys()
        .flatten()
        .find(|name| registered.open_subkey(name).is_ok())
}

/// What `install` writes under `root`: the plan, or nothing when a package
/// serves the menu, in the classic menu as well as Windows 11's.
fn wanted(menu: &Menu, root: &Root) -> (Vec<Key>, Option<String>) {
    match package(&menu.id, root) {
        Some(name) => (Vec::new(), Some(name)),
        None => (plan(menu), None),
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
pub(super) fn read(classes: &RegKey, path: &str, out: &mut Vec<Key>) -> std::io::Result<()> {
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
pub(super) fn sorted(mut keys: Vec<Key>) -> Vec<Key> {
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
    let (keys, package) = wanted(menu, root);
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
    report.notes.push(match package {
        Some(name) => format!(
            "Windows: the package {name} serves the menu, with the native extension, in \
             Windows 11's menu and under \"Show more options\"."
        ),
        None => "Windows: the items are in the classic menu, which Windows 11 shows under \
                 \"Show more options\"; Windows runs the program once for each file chosen."
            .into(),
    });
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
/// [`install`] writes it, and nothing else of the menu's: false too when a
/// package serving it came or went.
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
    Ok(sorted(there) == sorted(wanted(menu, root).0)
        && definition.is_ok_and(|text| text == menu.to_definition()))
}
