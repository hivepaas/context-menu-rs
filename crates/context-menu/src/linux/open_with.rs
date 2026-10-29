//! Linux: Open With, from a desktop entry in the user's `applications`
//! folder, `<id>.open-with.desktop`, listing the MIME types of the files
//! the handler opens. GLib (GNOME's file managers, Nemo, Thunar) and KDE
//! (Dolphin) list it in Open With for those types. It is hidden from the
//! app menus (`NoDisplay`): it is not the app's launcher.
//!
//! GLib finds the apps for a type through each folder's `mimeinfo.cache`,
//! not the entries themselves, so the handler is listed there too, among
//! the other apps' lines, as `update-desktop-database` would list it.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::quote::{exec, key_value};
use super::{Dirs, File, mime, remove, write};
use crate::open_with::Handler;
use crate::{Error, Report};

/// The cache's one group.
const GROUP: &str = "[MIME Cache]";

/// The name of the handler `id`'s desktop entry, which is also its id for
/// the system: `<id>.open-with.desktop`, as `xdg-mime default` takes it.
pub fn desktop_id(id: &str) -> String {
    format!("{id}.open-with.desktop")
}

fn dir(dirs: &Dirs) -> PathBuf {
    dirs.data_home.join("applications")
}

fn cache_path(dirs: &Dirs) -> PathBuf {
    dir(dirs).join("mimeinfo.cache")
}

/// Puts `handler` in Open With under `dirs`, in place of an earlier install.
pub fn install(handler: &Handler, dirs: &Dirs) -> Result<Report, Error> {
    handler.validate()?;
    let mut report = Report::default();
    let types = types(handler, dirs, &mut report.notes);
    let path = dir(dirs).join(desktop_id(&handler.id));
    if types.is_empty() {
        remove(&path, &mut report)?;
    } else {
        write(&File {
            path: path.clone(),
            text: entry(handler, &types),
            executable: false,
        })?;
        report.written.push(path);
    }
    // After the entry, so the cache never names one there is not.
    update_cache(&handler.id, &types, dirs, &mut report)?;
    Ok(report)
}

/// Takes the handler `id` out of Open With under `dirs`.
pub fn uninstall(id: &str, dirs: &Dirs) -> Result<Report, Error> {
    let mut report = Report::default();
    update_cache(id, &[], dirs, &mut report)?;
    remove(&dir(dirs).join(desktop_id(id)), &mut report)?;
    Ok(report)
}

/// Whether the entry and the cache under `dirs` hold `handler` as
/// [`install`] would write it.
pub fn is_current(handler: &Handler, dirs: &Dirs) -> Result<bool, Error> {
    handler.validate()?;
    let types = types(handler, dirs, &mut Vec::new());
    let path = dir(dirs).join(desktop_id(&handler.id));
    let entry_current = if types.is_empty() {
        !path.exists()
    } else {
        fs::read_to_string(&path).is_ok_and(|text| text == entry(handler, &types))
    };
    let text = read(&cache_path(dirs))?;
    Ok(entry_current && cache(text.as_deref(), &desktop_id(&handler.id), &types).is_none())
}

/// The MIME types of the files `handler` opens, each once, with a note for
/// each extension of no type known here.
fn types(handler: &Handler, dirs: &Dirs, notes: &mut Vec<String>) -> Vec<String> {
    let mut types = Vec::new();
    for ext in &handler.extensions {
        let found = mime::types_of(ext, &dirs.data_dirs);
        if found.is_empty() {
            notes.push(format!(
                "Linux: no MIME type is known here for .{ext}, so {:?} is not in Open With for \
                 those files.",
                handler.name
            ));
        }
        for t in found {
            if !types.contains(&t) {
                types.push(t);
            }
        }
    }
    types
}

fn entry(handler: &Handler, types: &[String]) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# Made by context-menu-rs for the handler {:?}. It is rewritten on install.",
        handler.id
    );
    s.push_str("[Desktop Entry]\nType=Application\n");
    let _ = writeln!(s, "Name={}", key_value(&handler.name));
    if let Some(icon) = &handler.icon {
        let _ = writeln!(s, "Icon={}", key_value(icon));
    }
    let _ = writeln!(
        s,
        "Exec={}",
        key_value(&exec(&handler.program, &handler.args))
    );
    let _ = writeln!(s, "MimeType={};", types.join(";"));
    // In Open With, not among the apps.
    s.push_str("NoDisplay=true\n");
    s
}

/// Lists the handler `id` in the cache for `types` alone, when it is not.
fn update_cache(id: &str, types: &[String], dirs: &Dirs, report: &mut Report) -> Result<(), Error> {
    let path = cache_path(dirs);
    let text = read(&path)?;
    let Some(text) = cache(text.as_deref(), &desktop_id(id), types) else {
        return Ok(());
    };
    write(&File {
        path: path.clone(),
        text,
        executable: false,
    })?;
    report.written.push(path);
    Ok(())
}

/// The cache `text`, or a new one, with the entry `desktop` listed for
/// `types` alone: after the other apps in the lines of those it has, and in
/// new lines for the others; out of every other line, and a line left with
/// no app gone. Every other line is kept as it is. None when nothing
/// changes.
fn cache(text: Option<&str>, desktop: &str, types: &[String]) -> Option<String> {
    let mut out = String::new();
    let mut changed = false;
    let mut listed: Vec<&str> = Vec::new();
    let mut in_group = false;
    let mut had_group = false;
    // The lines of the types not listed yet, at the end of the group.
    let rest = |out: &mut String, listed: &[&str]| {
        let mut added = false;
        for t in types.iter().filter(|t| !listed.contains(&t.as_str())) {
            let _ = writeln!(out, "{t}={desktop};");
            added = true;
        }
        added
    };
    for line in text.unwrap_or("").lines() {
        if line.starts_with('[') {
            if in_group {
                changed |= rest(&mut out, &listed);
            }
            in_group = line.trim_end() == GROUP;
            had_group |= in_group;
        } else if let Some((mime, apps)) = line.split_once('=').filter(|_| in_group) {
            let mime = mime.trim();
            let mut apps: Vec<&str> = apps.split(';').map(str::trim).collect();
            apps.retain(|a| !a.is_empty());
            let has = apps.contains(&desktop);
            let wanted = types.iter().any(|t| t == mime);
            if wanted {
                listed.push(mime);
            }
            if has != wanted {
                changed = true;
                apps.retain(|a| *a != desktop);
                if wanted {
                    apps.push(desktop);
                }
                if !apps.is_empty() {
                    let _ = writeln!(out, "{mime}={};", apps.join(";"));
                }
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_group {
        changed |= rest(&mut out, &listed);
    }
    if !had_group && !types.is_empty() {
        out.push_str(GROUP);
        out.push('\n');
        rest(&mut out, &listed);
        changed = true;
    }
    changed.then_some(out)
}

/// The text of `path`, or none when there is no such file.
fn read(path: &Path) -> Result<Option<String>, Error> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::testing::dirs;

    fn handler() -> Handler {
        Handler::new("packwing", "Pack Wing", "/opt/Pack Wing/packwing")
            .arg("--open")
            .icon("package-x-generic")
            .extensions(["zip", "tar.gz", "nope"])
    }

    fn names(paths: &[PathBuf], d: &Dirs) -> Vec<String> {
        paths
            .iter()
            .map(|p| p.strip_prefix(&d.data_home).unwrap().display().to_string())
            .collect()
    }

    #[test]
    fn a_handler_is_a_hidden_desktop_entry() {
        let d = dirs("open-with");
        let r = install(&handler(), &d).unwrap();
        assert_eq!(
            names(&r.written, &d),
            [
                "applications/packwing.open-with.desktop",
                "applications/mimeinfo.cache"
            ]
        );
        assert_eq!(
            r.notes,
            [
                "Linux: no MIME type is known here for .nope, so \"Pack Wing\" is not in Open \
                 With for those files."
            ]
        );
        let text = fs::read_to_string(&r.written[0]).unwrap();
        assert_eq!(
            text,
            "# Made by context-menu-rs for the handler \"packwing\". It is rewritten on install.\n\
             [Desktop Entry]\n\
             Type=Application\n\
             Name=Pack Wing\n\
             Icon=package-x-generic\n\
             Exec=\"/opt/Pack Wing/packwing\" --open %F\n\
             MimeType=application/zip;application/x-compressed-tar;\n\
             NoDisplay=true\n"
        );
        assert_eq!(
            fs::read_to_string(&r.written[1]).unwrap(),
            "[MIME Cache]\n\
             application/zip=packwing.open-with.desktop;\n\
             application/x-compressed-tar=packwing.open-with.desktop;\n"
        );
        assert!(is_current(&handler(), &d).unwrap());
        // Again: the cache as it was.
        assert_eq!(install(&handler(), &d).unwrap().written.len(), 1);
    }

    #[test]
    fn the_cache_keeps_the_other_apps() {
        let d = dirs("open-with-cache");
        let cache = cache_path(&d);
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        let theirs = "[MIME Cache]\n\
                      application/gzip=gzipper.desktop;\n\
                      application/zip=roller.desktop;ark.desktop;\n\
                      text/plain = editor.desktop ;\n";
        fs::write(&cache, theirs).unwrap();

        install(&handler(), &d).unwrap();
        assert_eq!(
            fs::read_to_string(&cache).unwrap(),
            "[MIME Cache]\n\
             application/gzip=gzipper.desktop;\n\
             application/zip=roller.desktop;ark.desktop;packwing.open-with.desktop;\n\
             text/plain = editor.desktop ;\n\
             application/x-compressed-tar=packwing.open-with.desktop;\n"
        );

        // For fewer files: out of the lines of the others, and a line with
        // no app left gone.
        let fewer = Handler {
            extensions: vec!["gz".into()],
            ..handler()
        };
        assert!(!is_current(&fewer, &d).unwrap());
        install(&fewer, &d).unwrap();
        assert_eq!(
            fs::read_to_string(&cache).unwrap(),
            "[MIME Cache]\n\
             application/gzip=gzipper.desktop;packwing.open-with.desktop;\n\
             application/zip=roller.desktop;ark.desktop;\n\
             text/plain = editor.desktop ;\n"
        );
        assert!(is_current(&fewer, &d).unwrap());

        let r = uninstall("packwing", &d).unwrap();
        assert_eq!(names(&r.written, &d), ["applications/mimeinfo.cache"]);
        assert_eq!(
            names(&r.removed, &d),
            ["applications/packwing.open-with.desktop"]
        );
        assert_eq!(
            fs::read_to_string(&cache).unwrap(),
            "[MIME Cache]\n\
             application/gzip=gzipper.desktop;\n\
             application/zip=roller.desktop;ark.desktop;\n\
             text/plain = editor.desktop ;\n"
        );
        let again = uninstall("packwing", &d).unwrap();
        assert!(again.written.is_empty() && again.removed.is_empty());
    }

    #[test]
    fn current_until_the_handler_or_the_files_change() {
        let d = dirs("open-with-current");
        assert!(!is_current(&handler(), &d).unwrap(), "nothing installed");
        install(&handler(), &d).unwrap();
        assert!(is_current(&handler(), &d).unwrap());
        let renamed = Handler {
            name: "packwing".into(),
            ..handler()
        };
        assert!(!is_current(&renamed, &d).unwrap());

        // Out of the cache, as by hand.
        let cache = cache_path(&d);
        let text = fs::read_to_string(&cache).unwrap();
        fs::write(
            &cache,
            text.replace("application/zip=packwing.open-with.desktop;\n", ""),
        )
        .unwrap();
        assert!(!is_current(&handler(), &d).unwrap());
        install(&handler(), &d).unwrap();
        assert!(is_current(&handler(), &d).unwrap());

        // For files of no type known here, there is nothing to write.
        let unknown = Handler {
            extensions: vec!["nope".into()],
            ..handler()
        };
        assert!(!is_current(&unknown, &d).unwrap());
        let r = install(&unknown, &d).unwrap();
        assert_eq!(
            names(&r.removed, &d),
            ["applications/packwing.open-with.desktop"]
        );
        assert!(is_current(&unknown, &d).unwrap());
        assert!(!fs::read_to_string(&cache).unwrap().contains("packwing"));
    }

    /// GLib lists the handler for its types, from a data folder of the test's
    /// own, and the system's tools take its entry as valid.
    #[test]
    fn glib_finds_it() {
        let d = dirs("open-with-glib");
        let program = std::env::current_exe().unwrap();
        let handler =
            Handler::new("context-menu-test", "context-menu test", &program).extensions(["zip"]);
        let r = install(&handler, &d).unwrap();
        if let Ok(out) = std::process::Command::new("desktop-file-validate")
            .arg(&r.written[0])
            .output()
        {
            assert!(out.status.success(), "{out:?}");
        }
        let Ok(out) = std::process::Command::new("gio")
            .args(["mime", "application/zip"])
            .env("XDG_DATA_HOME", &d.data_home)
            .output()
        else {
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("context-menu-test.open-with.desktop"),
            "{text}"
        );
    }
}
