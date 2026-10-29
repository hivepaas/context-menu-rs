//! Windows: Open With, from a ProgID in the user's part of the registry,
//! `ContextMenuRs.<id>`, whose `open` verb runs the program, listed in the
//! `OpenWithProgids` of each extension the handler is for. Explorer offers
//! the app in Open With for those files, in Windows 11's menu and the
//! classic one, and in "Choose another app". The ProgID's capabilities,
//! named in `RegisteredApplications`, put the app in Settings' Default
//! apps, where the user can make it the one that opens them. See
//! `docs/windows.md`.
//!
//! As for the menu, what goes in the registry is planned here in plain
//! Rust, the same on every system, and written on Windows alone.

use super::{Key, MARK, command, icon};
use crate::open_with::Handler;

/// The handler `id`'s ProgID, `ContextMenuRs.<id>`: what an extension
/// lists, and what Windows keeps as the app the user chose for it. It
/// names the handler in `RegisteredApplications` too.
pub fn prog_id(id: &str) -> String {
    format!("{MARK}.{id}")
}

/// The page of Settings' Default apps that is the handler `id`'s, where the
/// user can make it open the types it is for: what an app can open, as
/// `start ms-settings:…`, to ask them to. Windows 11's.
pub fn settings_uri(id: &str) -> String {
    format!("ms-settings:defaultapps?registeredAppUser={}", prog_id(id))
}

/// The extensions the handler is listed for, as Windows knows files: by
/// their last extension, `.gz` for `tar.gz`, each once.
pub(crate) fn extensions(handler: &Handler) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ext in &handler.extensions {
        let last = format!(".{}", ext.rsplit('.').next().unwrap_or(ext));
        if !out.contains(&last) {
            out.push(last);
        }
    }
    out
}

/// The keys of the handler's ProgID, under the classes key.
pub(crate) fn plan(handler: &Handler) -> Vec<Key> {
    let id = prog_id(&handler.id);
    let name = handler.name.clone();
    let icon = icon(handler.icon.as_ref());
    let key = |path: String, values: Vec<(&str, String)>| Key {
        path,
        values: values
            .into_iter()
            .map(|(n, v)| (n.to_string(), v))
            .collect(),
    };
    let mut keys = vec![key(id.clone(), vec![(MARK, handler.id.clone())])];
    // For the files the user made it open: theirs, with no icon of the
    // app's.
    if let Some(icon) = icon {
        keys.push(key(format!(r"{id}\DefaultIcon"), vec![("", icon.into())]));
    }
    keys.push(key(format!(r"{id}\shell"), Vec::new()));
    keys.push(key(
        format!(r"{id}\shell\open"),
        vec![("FriendlyAppName", name.clone())],
    ));
    keys.push(key(
        format!(r"{id}\shell\open\command"),
        vec![("", command(&handler.program, &handler.args))],
    ));
    let mut capabilities = vec![
        ("ApplicationName", name.clone()),
        ("ApplicationDescription", name),
    ];
    if let Some(icon) = icon {
        capabilities.push(("ApplicationIcon", icon.into()));
    }
    keys.push(key(format!(r"{id}\Capabilities"), capabilities));
    keys.push(Key {
        path: format!(r"{id}\Capabilities\FileAssociations"),
        values: extensions(handler)
            .into_iter()
            .map(|ext| (ext, id.clone()))
            .collect(),
    });
    keys
}

/// What the user may need to know of the handler on Windows.
pub(crate) fn notes(handler: &Handler) -> Vec<String> {
    let mut notes = vec![format!(
        "Windows: {:?} is in Open With for {}; Windows runs the program once for each file \
         chosen.",
        handler.name,
        extensions(handler).join(", ")
    )];
    for ext in &handler.extensions {
        let last = ext.rsplit('.').next().unwrap_or(ext);
        if last != ext && !handler.extensions.iter().any(|e| e == last) {
            notes.push(format!(
                "Windows: a file's type is its last extension, so {:?} is in Open With for \
                 every .{last} file, not .{ext} files alone.",
                handler.name
            ));
        }
    }
    notes
}

#[cfg(windows)]
pub use registry::{install, is_current, uninstall};

/// The plan written to the registry, and what is there read back.
#[cfg(windows)]
mod registry {
    use std::path::PathBuf;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};

    use super::{extensions, notes, plan, prog_id};
    use crate::open_with::Handler;
    use crate::windows::Root;
    use crate::windows::registry::{read, sorted};
    use crate::{Error, Report};

    fn error(path: String, source: std::io::Error) -> Error {
        Error::Io {
            path: PathBuf::from(path),
            source,
        }
    }

    /// What `RegisteredApplications` names for the ProgID `id`: its
    /// capabilities' key.
    fn capabilities(root: &Root, id: &str) -> String {
        format!(r"{}\{id}\Capabilities", root.classes)
    }

    /// The full name of the ProgID `id`'s value in `RegisteredApplications`.
    fn registered_name(root: &Root, id: &str) -> PathBuf {
        PathBuf::from(format!(r"HKEY_CURRENT_USER\{}\{id}", root.registered()))
    }

    /// The extensions whose `OpenWithProgids` list the ProgID `id`, as the
    /// registry names them.
    fn listed(classes: &RegKey, id: &str) -> Vec<String> {
        classes
            .enum_keys()
            .flatten()
            .filter(|name| name.starts_with('.'))
            .filter(|name| {
                classes
                    .open_subkey(format!(r"{name}\OpenWithProgids"))
                    .is_ok_and(|list| list.get_raw_value(id).is_ok())
            })
            .collect()
    }

    /// Takes the ProgID `id` out of the list of the extension `ext`; and the
    /// list, and the extension's key, when that leaves them empty.
    fn unlist(classes: &RegKey, ext: &str, id: &str) -> std::io::Result<()> {
        let list = format!(r"{ext}\OpenWithProgids");
        classes
            .open_subkey_with_flags(&list, KEY_ALL_ACCESS)?
            .delete_value(id)?;
        for path in [list.as_str(), ext] {
            let info = classes.open_subkey(path)?.query_info()?;
            if info.sub_keys > 0 || info.values > 0 {
                break;
            }
            classes.delete_subkey(path)?;
        }
        Ok(())
    }

    /// Puts `handler` in Open With under `root`, in place of an earlier
    /// install.
    pub fn install(handler: &Handler, root: &Root) -> Result<Report, Error> {
        handler.validate()?;
        let classes = root.open()?;
        let id = prog_id(&handler.id);
        let wanted = extensions(handler);
        let mut report = Report {
            notes: notes(handler),
            ..Report::default()
        };
        // An earlier install's ProgID is written again whole, and taken out
        // of the extensions it is no longer for.
        if classes.open_subkey(&id).is_ok() {
            classes
                .delete_subkey_all(&id)
                .map_err(|e| root.error(&id, e))?;
        }
        for ext in listed(&classes, &id) {
            if !wanted.iter().any(|w| w.eq_ignore_ascii_case(&ext)) {
                let list = format!(r"{ext}\OpenWithProgids");
                unlist(&classes, &ext, &id).map_err(|e| root.error(&list, e))?;
                report.removed.push(root.name(&format!(r"{list}\{id}")));
            }
        }
        for key in &plan(handler) {
            let (k, _) = classes
                .create_subkey(&key.path)
                .map_err(|e| root.error(&key.path, e))?;
            for (name, value) in &key.values {
                k.set_value(name, value)
                    .map_err(|e| root.error(&key.path, e))?;
            }
        }
        report.written.push(root.name(&id));
        // In each extension's list, after the ProgID is there.
        for ext in &wanted {
            let list = format!(r"{ext}\OpenWithProgids");
            classes
                .create_subkey(&list)
                .and_then(|(k, _)| k.set_value(&id, &String::new()))
                .map_err(|e| root.error(&list, e))?;
            report.written.push(root.name(&format!(r"{list}\{id}")));
        }
        let registered = root.registered();
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&registered)
            .and_then(|(k, _)| k.set_value(&id, &capabilities(root, &id)))
            .map_err(|e| error(format!(r"HKEY_CURRENT_USER\{registered}"), e))?;
        report.written.push(registered_name(root, &id));
        Ok(report)
    }

    /// Takes the handler `id` out of Open With under `root`.
    pub fn uninstall(id: &str, root: &Root) -> Result<Report, Error> {
        let id = prog_id(id);
        let mut report = Report::default();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(classes) = hkcu.open_subkey_with_flags(&root.classes, KEY_ALL_ACCESS) {
            for ext in listed(&classes, &id) {
                let list = format!(r"{ext}\OpenWithProgids");
                unlist(&classes, &ext, &id).map_err(|e| root.error(&list, e))?;
                report.removed.push(root.name(&format!(r"{list}\{id}")));
            }
            if classes.open_subkey(&id).is_ok() {
                classes
                    .delete_subkey_all(&id)
                    .map_err(|e| root.error(&id, e))?;
                report.removed.push(root.name(&id));
            }
        }
        let registered = root.registered();
        if let Ok(k) = hkcu.open_subkey_with_flags(&registered, KEY_ALL_ACCESS)
            && k.get_raw_value(&id).is_ok()
        {
            k.delete_value(&id)
                .map_err(|e| error(format!(r"HKEY_CURRENT_USER\{registered}"), e))?;
            report.removed.push(registered_name(root, &id));
        }
        Ok(report)
    }

    /// Whether the registry under `root` holds `handler` as [`install`]
    /// writes it: its ProgID, the extensions listing it and no others, and
    /// its capabilities registered.
    pub fn is_current(handler: &Handler, root: &Root) -> Result<bool, Error> {
        handler.validate()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(classes) = hkcu.open_subkey(&root.classes) else {
            return Ok(false);
        };
        let id = prog_id(&handler.id);
        if classes.open_subkey(&id).is_err() {
            return Ok(false);
        }
        let mut there = Vec::new();
        read(&classes, &id, &mut there).map_err(|e| root.error(&id, e))?;
        let mut listed: Vec<String> = listed(&classes, &id)
            .iter()
            .map(|ext| ext.to_ascii_lowercase())
            .collect();
        listed.sort();
        let mut wanted = extensions(handler);
        wanted.sort();
        let registered = hkcu
            .open_subkey(root.registered())
            .and_then(|k| k.get_value::<String, _>(&id))
            .is_ok_and(|v| v == capabilities(root, &id));
        Ok(sorted(there) == sorted(plan(handler)) && listed == wanted && registered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = r"C:\Program Files\Pack Wing\packwing.exe";

    fn handler() -> Handler {
        Handler::new("packwing", "Pack Wing", APP)
            .arg("--open")
            .icon(r"C:\Program Files\Pack Wing\packwing.exe,0")
            .extensions(["zip", "tar.gz", "tgz"])
    }

    #[test]
    fn a_handler_is_a_prog_id() {
        assert_eq!(prog_id("packwing"), "ContextMenuRs.packwing");
        assert_eq!(
            settings_uri("photo-kit"),
            "ms-settings:defaultapps?registeredAppUser=ContextMenuRs.photo-kit"
        );
        assert_eq!(extensions(&handler()), [".zip", ".gz", ".tgz"]);
        let keys = plan(&handler());
        let paths: Vec<&str> = keys.iter().map(|k| k.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                r"ContextMenuRs.packwing",
                r"ContextMenuRs.packwing\DefaultIcon",
                r"ContextMenuRs.packwing\shell",
                r"ContextMenuRs.packwing\shell\open",
                r"ContextMenuRs.packwing\shell\open\command",
                r"ContextMenuRs.packwing\Capabilities",
                r"ContextMenuRs.packwing\Capabilities\FileAssociations",
            ]
        );
        let values = |i: usize| -> Vec<(&str, &str)> {
            keys[i]
                .values
                .iter()
                .map(|(n, v)| (n.as_str(), v.as_str()))
                .collect()
        };
        assert_eq!(values(0), [("ContextMenuRs", "packwing")]);
        assert_eq!(
            values(1),
            [("", r"C:\Program Files\Pack Wing\packwing.exe,0")]
        );
        assert!(values(2).is_empty());
        assert_eq!(values(3), [("FriendlyAppName", "Pack Wing")]);
        assert_eq!(
            values(4),
            [(
                "",
                r#""C:\Program Files\Pack Wing\packwing.exe" "--open" "%1""#
            )]
        );
        assert_eq!(
            values(5),
            [
                ("ApplicationName", "Pack Wing"),
                ("ApplicationDescription", "Pack Wing"),
                (
                    "ApplicationIcon",
                    r"C:\Program Files\Pack Wing\packwing.exe,0"
                ),
            ]
        );
        assert_eq!(
            values(6),
            [
                (".zip", "ContextMenuRs.packwing"),
                (".gz", "ContextMenuRs.packwing"),
                (".tgz", "ContextMenuRs.packwing"),
            ]
        );
    }

    #[test]
    fn a_theme_icon_is_left_out_and_longer_extensions_noted() {
        let h = Handler::new("packwing", "Pack Wing", APP)
            .icon("package-x-generic")
            .extensions(["tar.gz", "tar.zst", "zst"]);
        let keys = plan(&h);
        assert!(!keys.iter().any(|k| k.path.ends_with("DefaultIcon")));
        assert!(!keys[4].values.iter().any(|(n, _)| n == "ApplicationIcon"));
        assert_eq!(
            notes(&h),
            [
                "Windows: \"Pack Wing\" is in Open With for .gz, .zst; Windows runs the program \
                 once for each file chosen.",
                "Windows: a file's type is its last extension, so \"Pack Wing\" is in Open With \
                 for every .gz file, not .tar.gz files alone.",
            ]
        );
    }
}
