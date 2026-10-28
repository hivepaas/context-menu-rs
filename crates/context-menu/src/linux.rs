//! Linux: Nautilus (GNOME Files), Dolphin and Nemo, each from files in the
//! user's data folder (`$XDG_DATA_HOME`, `~/.local/share`), and Thunar from
//! its custom actions in the user's config folder. Every file manager gets
//! its files, installed or not, so the menu is there when one is; and the
//! menu's definition goes in `context-menu-rs/<id>.menu`, for the native
//! extensions. See `docs/linux.md`.

mod dolphin;
mod mime;
mod nautilus;
mod nemo;
mod quote;
mod thunar;

pub use nautilus::{Way, native_name};

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
    /// The folders Nautilus loads its native extensions from, those there
    /// are: the menu's own extension or nautilus-python may be in one.
    pub nautilus_extensions: Vec<PathBuf>,
    /// The user's config folder: `$XDG_CONFIG_HOME`, or `~/.config`.
    pub config_home: PathBuf,
    /// The system's config folders, `$XDG_CONFIG_DIRS`: Thunar's sample
    /// actions are in one.
    pub config_dirs: Vec<PathBuf>,
}

impl Dirs {
    /// The current user's, from the environment.
    pub fn current() -> Result<Self, Error> {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        let data_home = data_home()?;
        let shared = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let mut data_dirs = vec![data_home.clone()];
        data_dirs.extend(std::env::split_paths(&shared).filter(|d| d.is_absolute()));
        let config_home = match var("XDG_CONFIG_HOME") {
            Some(d) if Path::new(&d).is_absolute() => PathBuf::from(d),
            _ => std::env::home_dir()
                .filter(|h| h.is_absolute())
                .ok_or(Error::NoHome)?
                .join(".config"),
        };
        let config_dirs = var("XDG_CONFIG_DIRS").unwrap_or_else(|| "/etc/xdg".into());
        Ok(Dirs {
            data_home,
            data_dirs,
            nautilus_extensions: nautilus::extension_dirs(),
            config_home,
            config_dirs: std::env::split_paths(&config_dirs)
                .filter(|d| d.is_absolute())
                .collect(),
        })
    }
}

/// The current user's data folder: `$XDG_DATA_HOME`, or `~/.local/share`.
pub fn data_home() -> Result<PathBuf, Error> {
    match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(d) if Path::new(&d).is_absolute() => Ok(PathBuf::from(d)),
        _ => Ok(std::env::home_dir()
            .filter(|h| h.is_absolute())
            .ok_or(Error::NoHome)?
            .join(".local/share")),
    }
}

/// Where the definition of the menu `id` goes in the data folder
/// `data_home`: what the native extensions read.
pub fn definition_path(id: &str, data_home: &Path) -> PathBuf {
    data_home.join("context-menu-rs").join(format!("{id}.menu"))
}

/// Installs `menu` under `dirs`, in place of an earlier install: what that
/// wrote and this one does not is removed.
pub fn install(menu: &Menu, dirs: &Dirs) -> Result<Report, Error> {
    menu.validate()?;
    let (files, notes) = plan(menu, dirs);
    let mut report = Report {
        notes,
        ..Report::default()
    };
    for path in existing(&menu.id, dirs)? {
        if !files.iter().any(|f| f.path == path) {
            remove(&path, &mut report)?;
        }
    }
    nautilus::tidy(&menu.id, dirs, &report.removed, false)?;
    for f in files {
        write(&f)?;
        report.written.push(f.path);
    }
    thunar::update(&menu.id, Some(menu), dirs, &mut report)?;
    Ok(report)
}

/// Takes the menu `id` away from under `dirs`: the files an install of it
/// wrote, found by their names and by the mark in them.
pub fn uninstall(id: &str, dirs: &Dirs) -> Result<Report, Error> {
    let mut report = Report::default();
    for path in existing(id, dirs)? {
        remove(&path, &mut report)?;
    }
    nautilus::tidy(id, dirs, &report.removed, true)?;
    thunar::update(id, None, dirs, &mut report)?;
    Ok(report)
}

/// Whether what [`install`] would write under `dirs` is there as it would
/// write it, and nothing else of the menu's.
pub fn is_current(menu: &Menu, dirs: &Dirs) -> Result<bool, Error> {
    menu.validate()?;
    let (files, _) = plan(menu, dirs);
    let mut there = existing(&menu.id, dirs)?;
    there.sort();
    let mut planned: Vec<&Path> = files.iter().map(|f| f.path.as_path()).collect();
    planned.sort();
    if there
        .iter()
        .map(PathBuf::as_path)
        .ne(planned.iter().copied())
    {
        return Ok(false);
    }
    for f in &files {
        let Ok(text) = fs::read_to_string(&f.path) else {
            return Ok(false);
        };
        let mode = fs::metadata(&f.path)
            .map_err(|e| Error::io(&f.path, e))?
            .permissions()
            .mode();
        if text != f.text || (mode & 0o111 != 0) != f.executable {
            return Ok(false);
        }
    }
    thunar::is_current(menu, dirs)
}

/// What an install of `menu` writes, and its notes.
fn plan(menu: &Menu, dirs: &Dirs) -> (Vec<File>, Vec<String>) {
    let mut files = vec![File {
        path: definition_path(&menu.id, &dirs.data_home),
        text: menu.to_definition(),
        executable: false,
    }];
    let mut notes = Vec::new();
    dolphin::files(menu, dirs, &mut files, &mut notes);
    nemo::files(menu, dirs, &mut files, &mut notes);
    nautilus::files(menu, dirs, &mut files, &mut notes);
    (files, notes)
}

/// The files of the menu `id` there are now.
fn existing(id: &str, dirs: &Dirs) -> Result<Vec<PathBuf>, Error> {
    let mut found = Vec::new();
    let definition = definition_path(id, &dirs.data_home);
    if definition.is_file() {
        found.push(definition);
    }
    found.extend(dolphin::existing(id, dirs)?);
    found.extend(nemo::existing(id, dirs)?);
    found.extend(nautilus::existing(id, dirs)?);
    Ok(found)
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

/// The files in `dir` named `{id}.*{suffix}`. Ids hold no `.`, so another
/// menu's never match.
fn named(dir: &Path, id: &str, suffix: &str) -> Result<Vec<PathBuf>, Error> {
    let prefix = format!("{id}.");
    Ok(list(dir)?
        .into_iter()
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(suffix)
        })
        .collect())
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

    /// Dirs in a scratch folder: a MIME database that knows zip and 7z,
    /// and an empty folder of Nautilus extensions.
    pub fn dirs(name: &str) -> Dirs {
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
        let extensions = root.join("extensions-4");
        std::fs::create_dir_all(&extensions).unwrap();
        Dirs {
            data_dirs: vec![data_home.clone(), shared],
            data_home,
            nautilus_extensions: vec![extensions],
            config_home: root.join("config"),
            config_dirs: vec![root.join("xdg")],
        }
    }

    /// Puts a library named `name` among the Nautilus extensions.
    pub fn add_extension(dirs: &Dirs, name: &str) {
        std::fs::write(dirs.nautilus_extensions[0].join(name), "").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{add_extension, dirs};
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

    /// Paths as names under the data folder, or `config/` and a name under
    /// the config folder.
    fn names(paths: &[PathBuf], d: &Dirs) -> Vec<String> {
        let mut v: Vec<String> = paths
            .iter()
            .map(|p| match p.strip_prefix(&d.data_home) {
                Ok(name) => name.display().to_string(),
                Err(_) => format!(
                    "config/{}",
                    p.strip_prefix(&d.config_home).unwrap().display()
                ),
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn install_writes_each_file_manager_its_files() {
        for way in [Way::Scripts, Way::Python, Way::Native] {
            let d = dirs("install");
            match way {
                Way::Python => add_extension(&d, "libnautilus-python.so"),
                Way::Native => add_extension(&d, &native_name("packwing")),
                Way::Scripts => {}
            }
            let r = install(&menu(true), &d).unwrap();
            let mut want = vec![
                "config/Thunar/uca.xml",
                "context-menu-rs/packwing.menu",
                "kio/servicemenus/packwing.compare.desktop",
                "kio/servicemenus/packwing.compress.desktop",
                "kio/servicemenus/packwing.extract-here.desktop",
                "nemo/actions/packwing.compare.nemo_action",
                "nemo/actions/packwing.compress.nemo_action",
                "nemo/actions/packwing.extract-here.nemo_action",
            ];
            match way {
                Way::Python => want.push("nautilus-python/extensions/packwing_context_menu.py"),
                Way::Scripts => want.extend([
                    "nautilus/scripts/packwing/Compare",
                    "nautilus/scripts/packwing/Compress…",
                    "nautilus/scripts/packwing/Extract here",
                ]),
                // The extension reads the definition.
                Way::Native => {}
            }
            want.sort();
            assert_eq!(names(&r.written, &d), want, "{way:?}");
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
            let definition = fs::read_to_string(definition_path("packwing", &d.data_home)).unwrap();
            assert_eq!(Menu::from_definition(&definition).unwrap(), menu(true));
        }
    }

    #[test]
    fn installing_again_replaces_and_uninstall_takes_all_away() {
        let d = dirs("again");
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
            names(&r.removed, &d),
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
        assert_eq!(r.removed.len(), 7);
        assert!(other.exists() && theirs.exists());
        assert!(uninstall("packwing", &d).unwrap().removed.is_empty());
    }

    #[test]
    fn current_until_the_menu_or_the_system_changes() {
        let d = dirs("current");
        assert!(!is_current(&menu(true), &d).unwrap(), "nothing installed");
        install(&menu(true), &d).unwrap();
        assert!(is_current(&menu(true), &d).unwrap());
        assert!(!is_current(&menu(false), &d).unwrap(), "the menu changed");

        // nautilus-python arrives: the scripts should give way.
        add_extension(&d, "libnautilus-python.so");
        assert!(!is_current(&menu(true), &d).unwrap());
        install(&menu(true), &d).unwrap();
        assert!(is_current(&menu(true), &d).unwrap());
        assert!(!d.data_home.join("nautilus/scripts/packwing").exists());

        // A file changed by hand, or one made not executable.
        let desktop = d
            .data_home
            .join("kio/servicemenus/packwing.compare.desktop");
        fs::set_permissions(&desktop, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_current(&menu(true), &d).unwrap());
        install(&menu(true), &d).unwrap();
        fs::write(&desktop, "changed").unwrap();
        assert!(!is_current(&menu(true), &d).unwrap());
    }
}
