//! Linux: Nautilus (GNOME Files), Dolphin and Nemo, each from files in the
//! user's data folder (`$XDG_DATA_HOME`, `~/.local/share`). Every file
//! manager gets its files, installed or not, so the menu is there when one
//! is. See `docs/linux.md`.

mod dolphin;
mod mime;
mod nautilus;
mod nemo;
mod quote;

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use crate::{Error, Menu, Report};

/// Where the menu's files go, and what is known of the system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// The user's data folder: `$XDG_DATA_HOME`, or `~/.local/share`.
    pub data_home: PathBuf,
    /// Where shared-mime-info's databases are, the user's first:
    /// `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`. Dolphin chooses by MIME type,
    /// so extensions are looked up in them.
    pub data_dirs: Vec<PathBuf>,
    /// nautilus-python is installed: Nautilus gets an extension, with the
    /// items in its menu, rather than scripts in its Scripts submenu.
    pub nautilus_python: bool,
}

impl Dirs {
    /// The current user's, from the environment.
    pub fn current() -> Result<Self, Error> {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        let data_home = match var("XDG_DATA_HOME") {
            Some(d) if Path::new(&d).is_absolute() => PathBuf::from(d),
            _ => std::env::home_dir()
                .filter(|h| h.is_absolute())
                .ok_or(Error::NoHome)?
                .join(".local/share"),
        };
        let shared = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let mut data_dirs = vec![data_home.clone()];
        data_dirs.extend(std::env::split_paths(&shared).filter(|d| d.is_absolute()));
        Ok(Dirs {
            data_home,
            data_dirs,
            nautilus_python: nautilus::has_python(),
        })
    }
}

/// Installs `menu` under `dirs`, in place of an earlier install.
pub fn install(menu: &Menu, dirs: &Dirs) -> Result<Report, Error> {
    menu.validate()?;
    let mut report = uninstall(&menu.id, dirs)?;
    let mut files = Vec::new();
    dolphin::files(menu, dirs, &mut files, &mut report.notes);
    nemo::files(menu, dirs, &mut files, &mut report.notes);
    nautilus::files(menu, dirs, &mut files, &mut report.notes);
    for f in files {
        write(&f)?;
        // What this install writes again is not gone.
        report.removed.retain(|p| p != &f.path);
        report.written.push(f.path);
    }
    Ok(report)
}

/// Takes the menu `id` away from under `dirs`: the files an install of it
/// wrote, found by their names and by the mark in them.
pub fn uninstall(id: &str, dirs: &Dirs) -> Result<Report, Error> {
    let mut report = Report::default();
    dolphin::remove(id, dirs, &mut report)?;
    nemo::remove(id, dirs, &mut report)?;
    nautilus::remove(id, dirs, &mut report)?;
    Ok(report)
}

/// A file an install writes.
struct File {
    path: PathBuf,
    text: String,
    /// It is run, or must be executable to count (Dolphin's).
    executable: bool,
}

/// What every file starts with, in a comment, so an uninstall knows it.
fn mark(id: &str) -> String {
    format!("Made by context-menu-rs for the menu {id:?}. It is rewritten on install.")
}

/// Writes `f` whole or not at all: to a hidden file beside it first, then
/// renamed over it, so a file manager never reads half of it.
fn write(f: &File) -> Result<(), Error> {
    let dir = f.path.parent().unwrap_or(Path::new("/"));
    fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let name = f.path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.tmp"));
    let result = (|| {
        let mut out = fs::File::create(&tmp)?;
        out.write_all(f.text.as_bytes())?;
        out.sync_all()?;
        let mode = if f.executable { 0o755 } else { 0o644 };
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        fs::rename(&tmp, &f.path)
    })();
    result.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        Error::io(&f.path, e)
    })
}

/// Removes the files in `dir` named `{id}.*{suffix}`. Ids hold no `.`, so
/// another menu's never match.
fn remove_named(dir: &Path, id: &str, suffix: &str, report: &mut Report) -> Result<(), Error> {
    let prefix = format!("{id}.");
    for path in list(dir)? {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with(&prefix) && name.ends_with(suffix) {
            remove(&path, report)?;
        }
    }
    Ok(())
}

/// The entries of `dir`, or none when it does not exist.
fn list(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    match fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|e| e.map(|e| e.path()).map_err(|err| Error::io(dir, err)))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::io(dir, e)),
    }
}

fn remove(path: &Path, report: &mut Report) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => {
            report.removed.push(path.to_path_buf());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path, e)),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::Dirs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh folder under the system's temporary one, for one test.
    pub fn scratch(name: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "context-menu-test-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Dirs in a scratch folder, with a MIME database that knows zip and 7z.
    pub fn dirs(name: &str, nautilus_python: bool) -> Dirs {
        let root = scratch(name);
        let data_home = root.join("data");
        let shared = root.join("shared");
        std::fs::create_dir_all(shared.join("mime")).unwrap();
        std::fs::write(
            shared.join("mime/globs2"),
            "# a comment\n\
             50:application/zip:*.zip\n\
             50:application/x-7z-compressed:*.7z\n\
             50:application/x-compressed-tar:*.tar.gz\n\
             50:application/gzip:*.gz\n\
             10:application/x-weird:*.ZIP:cs\n",
        )
        .unwrap();
        Dirs {
            data_dirs: vec![data_home.clone(), shared],
            data_home,
            nautilus_python,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::dirs;
    use super::*;
    use crate::{Count, Item};

    fn menu(submenu: bool) -> Menu {
        Menu::new("packwing", "packwing")
            .submenu(submenu)
            .item(
                Item::new("extract-here", "Extract here", "/usr/bin/packwing")
                    .arg("--extract-here")
                    .extensions(["zip", "7z"]),
            )
            .item(
                Item::new("compress", "Compress…", "/usr/bin/packwing")
                    .arg("--compress")
                    .folders(true),
            )
            .item(
                Item::new("compare", "Compare", "/usr/bin/packwing")
                    .arg("--compare")
                    .count(Count::Many),
            )
    }

    fn names(paths: &[PathBuf], root: &Path) -> Vec<String> {
        let mut v: Vec<String> = paths
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().display().to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn install_writes_each_file_manager_its_files() {
        for python in [false, true] {
            let d = dirs("install", python);
            let r = install(&menu(true), &d).unwrap();
            let mut want = vec![
                "kio/servicemenus/packwing.compare.desktop",
                "kio/servicemenus/packwing.compress.desktop",
                "kio/servicemenus/packwing.extract-here.desktop",
                "nemo/actions/packwing.compare.nemo_action",
                "nemo/actions/packwing.compress.nemo_action",
                "nemo/actions/packwing.extract-here.nemo_action",
            ];
            if python {
                want.push("nautilus-python/extensions/packwing_context_menu.py");
            } else {
                want.extend([
                    "nautilus/scripts/packwing/Compare",
                    "nautilus/scripts/packwing/Compress…",
                    "nautilus/scripts/packwing/Extract here",
                ]);
            }
            want.sort();
            assert_eq!(names(&r.written, &d.data_home), want);
            assert!(r.removed.is_empty());
            let mode = |p: &str| {
                fs::metadata(d.data_home.join(p))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(mode("kio/servicemenus/packwing.compare.desktop"), 0o755);
            assert_eq!(mode("nemo/actions/packwing.compare.nemo_action"), 0o644);
        }
    }

    #[test]
    fn installing_again_replaces_and_uninstall_takes_all_away() {
        let d = dirs("again", false);
        install(&menu(true), &d).unwrap();
        // Another app's files stay.
        let other = d.data_home.join("nemo/actions/packwingx.thing.nemo_action");
        fs::write(&other, "x").unwrap();
        let theirs = d.data_home.join("nautilus/scripts/My script");
        fs::write(&theirs, "#!/bin/sh\n").unwrap();

        // One item fewer: its files go, the rest are written again.
        let mut smaller = menu(false);
        smaller.items.pop();
        let r = install(&smaller, &d).unwrap();
        assert_eq!(
            names(&r.removed, &d.data_home),
            [
                "kio/servicemenus/packwing.compare.desktop",
                "nautilus/scripts/packwing/Compare",
                "nautilus/scripts/packwing/Compress…",
                "nautilus/scripts/packwing/Extract here",
                "nemo/actions/packwing.compare.nemo_action",
            ]
        );
        assert!(!d.data_home.join("nautilus/scripts/packwing").exists());
        assert!(d.data_home.join("nautilus/scripts/Extract here").exists());

        let r = uninstall("packwing", &d).unwrap();
        assert_eq!(r.removed.len(), 6);
        assert!(other.exists() && theirs.exists());
        assert!(uninstall("packwing", &d).unwrap().removed.is_empty());
    }
}
