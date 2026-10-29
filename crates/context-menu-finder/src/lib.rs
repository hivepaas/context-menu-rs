//! A Finder Sync extension that shows a context-menu-rs menu in Finder's
//! own right-click menu: in a submenu of its own, right after Quick Actions,
//! and each item only for what it applies to.
//!
//! It is one program, built once, that an app's bundle holds twice
//! (`context_menu::macos::FinderExtension`): as the extension, which Finder
//! runs in the sandbox, and as its helper, an app with no window, which
//! starts the items' programs, since the sandbox lets the extension start
//! none. Their `Info.plist`s name the menu they serve and the app group they
//! share. Each time Finder asks what to show, the extension reads the menu's
//! definition, which `context_menu::install` writes in
//! `~/Library/Application Support/context-menu-rs`, so a changed menu needs
//! no new app. An item chosen, it leaves a request in the app group's
//! folder, which only the app's own programs can write, and opens the
//! helper, which takes the request and starts the item's program. See
//! `docs/macos.md`.
//!
//! The system's interfaces are Objective-C: `src/ffi.rs` holds all of that,
//! and the unsafe code with it. What is decided, from what shows for the
//! files chosen to what runs, is plain Rust here, and tested on every
//! system.

// ffi.rs, which uses the rest, is macOS's alone.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(target_os = "macos")]
mod ffi;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use context_menu::{Chosen, Item, Menu};

/// Runs the program as the bundle it is in makes it: the extension, under
/// Finder, or the helper.
pub fn run() -> std::process::ExitCode {
    #[cfg(target_os = "macos")]
    {
        ffi::run()
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("context-menu-finder is for macOS's Finder");
        std::process::ExitCode::from(2)
    }
}

/// A file or folder chosen in Finder.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Picked {
    name: String,
    /// A folder, not one of the packages Finder shows as files.
    folder: bool,
    path: PathBuf,
}

impl Picked {
    /// The file or folder at `path`, a folder as `folder` says.
    fn at(path: PathBuf, folder: bool) -> Option<Picked> {
        Some(Picked {
            name: path.file_name()?.to_str()?.to_string(),
            folder,
            path,
        })
    }
}

/// The items that show for the files picked, in the menu's order, with
/// their places in it.
fn shown<'m>(menu: &'m Menu, picked: &[Picked]) -> Vec<(usize, &'m Item)> {
    let chosen: Vec<Chosen<'_>> = picked
        .iter()
        .map(|p| Chosen {
            name: &p.name,
            folder: p.folder,
        })
        .collect();
    menu.items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.applies_to.accepts(&chosen))
        .collect()
}

/// An item chosen, which the extension leaves for the helper: the item's
/// id, and the paths chosen. Not the program: the helper takes that from
/// the menu's definition.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Request {
    item: String,
    paths: Vec<PathBuf>,
}

/// A request's first line; another is refused.
const HEADER: &str = "context-menu-rs request 1";

impl Request {
    /// As text: the header, then `item` and a `path` per path, each a key, a
    /// tab and a value, escaped as in a menu's definition. None when a path
    /// is not UTF-8, which macOS's own file systems never have.
    fn to_text(&self) -> Option<String> {
        let mut s = format!("{HEADER}\nitem\t{}\n", escape(&self.item));
        for path in &self.paths {
            s.push_str(&format!("path\t{}\n", escape(path.to_str()?)));
        }
        Some(s)
    }

    /// The request in `text`, or none.
    fn from_text(text: &str) -> Option<Request> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let mut item = None;
        let mut paths = Vec::new();
        for line in lines {
            let (key, value) = line.split_once('\t')?;
            match key {
                "item" => item = Some(unescape(value)?),
                "path" => paths.push(PathBuf::from(unescape(value)?)),
                _ => {}
            }
        }
        Some(Request { item: item?, paths })
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next()? {
            '\\' => '\\',
            't' => '\t',
            'n' => '\n',
            _ => return None,
        });
    }
    Some(out)
}

/// Where requests wait in the app group's folder `group`.
fn requests(group: &Path) -> PathBuf {
    group.join("requests")
}

/// How long a request waits for the helper before it is dropped: the helper
/// is opened at once, so an older one is not for now.
const STALE: Duration = Duration::from_secs(60);

/// Leaves `request` in `dir`, written beside its name first and then
/// renamed, so the helper never reads half of one.
fn leave(dir: &Path, request: &Request) -> io::Result<PathBuf> {
    static N: AtomicUsize = AtomicUsize::new(0);
    let text = request
        .to_text()
        .ok_or_else(|| io::Error::other("a path is not UTF-8"))?;
    fs::create_dir_all(dir)?;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!(
        "{}-{nanos}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    );
    let tmp = dir.join(format!(".{name}.tmp"));
    let path = dir.join(format!("{name}.request"));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    Ok(path)
}

/// Takes the requests waiting in `dir`, oldest first: each claimed by
/// renaming it, so that two helpers never take one twice, then read and
/// removed. Those older than [`STALE`] at `now`, unreadable or not
/// understood are removed too, and so are files left half written.
fn take(dir: &Path, now: SystemTime) -> Vec<Request> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(now);
            (modified, e.path())
        })
        .collect();
    found.sort();
    let mut taken = Vec::new();
    for (modified, path) in found {
        let stale = now.duration_since(modified).unwrap_or_default() > STALE;
        if path.extension().is_none_or(|e| e != "request") {
            if stale {
                let _ = fs::remove_file(&path);
            }
            continue;
        }
        let claimed = path.with_extension(format!("claimed-{}", std::process::id()));
        if fs::rename(&path, &claimed).is_err() {
            continue;
        }
        let request = fs::read_to_string(&claimed)
            .ok()
            .and_then(|text| Request::from_text(&text));
        let _ = fs::remove_file(&claimed);
        if let Some(request) = request.filter(|_| !stale) {
            taken.push(request);
        }
    }
    taken
}

/// What a request runs: its item's program, with the item's arguments and
/// then the paths, in the first one's folder, in a process group of its
/// own, with nothing to read from it or write to it, as Finder would start
/// it. None when the menu has no such item, or the paths are not what it
/// applies to: absolute, there, and of the kinds it is for.
fn command(menu: &Menu, request: &Request) -> Option<std::process::Command> {
    let item = menu.items.iter().find(|i| i.id == request.item)?;
    let picked: Option<Vec<Picked>> = request
        .paths
        .iter()
        .map(|p| {
            let meta = p.is_absolute().then(|| fs::metadata(p).ok())??;
            Picked::at(p.clone(), meta.is_dir() && !is_package(p))
        })
        .collect();
    let picked = picked?;
    let chosen: Vec<Chosen<'_>> = picked
        .iter()
        .map(|p| Chosen {
            name: &p.name,
            folder: p.folder,
        })
        .collect();
    if !item.applies_to.accepts(&chosen) {
        return None;
    }
    let mut run = std::process::Command::new(&item.program);
    run.args(&item.args)
        .args(&request.paths)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(folder) = request.paths.first().and_then(|p| p.parent()) {
        run.current_dir(folder);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut run, 0);
    Some(run)
}

/// Whether the folder at `path` is one Finder shows as a file: an app, or a
/// document that is a package. The extension asks the system; the helper,
/// checking a request, goes by what all of them have, an extension.
fn is_package(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        ffi::is_package(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// The menu `id`, as its definition in the data folder of the user whose
/// home is `home` says now.
fn load(id: &str, home: &Path) -> Result<Menu, context_menu::Error> {
    let path = context_menu::macos::definition_path(id, &home.join("Library/Application Support"));
    let text =
        fs::read_to_string(&path).map_err(|source| context_menu::Error::Io { path, source })?;
    Menu::from_definition(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use context_menu::Count;

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "context-menu-finder-test-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An absolute path on this system: Windows's have a drive.
    const APP: &str = if cfg!(windows) {
        r"C:\Photo Kit\photokit.exe"
    } else {
        "/Applications/Photo Kit.app/Contents/MacOS/photokit"
    };

    fn menu() -> Menu {
        Menu::new("photo-kit", "Photo Kit")
            .submenu(true)
            .item(
                Item::new("archives", "Archives", APP)
                    .args(["--open"])
                    .extensions(["zip", "tar.gz"]),
            )
            .item(Item::new("two", "Compare", APP).count(Count::Many))
            .item(
                Item::new("folder", "Pack", APP)
                    .folders_only()
                    .count(Count::One),
            )
    }

    fn pick(name: &str, folder: bool) -> Picked {
        Picked {
            name: name.into(),
            folder,
            path: PathBuf::from("/Users/u/Desktop").join(name),
        }
    }

    #[test]
    fn items_show_for_what_they_apply_to() {
        let m = menu();
        let ids = |picked: &[Picked]| -> Vec<&str> {
            shown(&m, picked)
                .iter()
                .map(|(_, i)| i.id.as_str())
                .collect()
        };
        assert_eq!(ids(&[pick("a.zip", false)]), ["archives"]);
        assert_eq!(
            ids(&[pick("a.ZIP", false), pick("b.tar.gz", false)]),
            ["archives", "two"]
        );
        assert!(ids(&[pick("b.gz", false)]).is_empty());
        assert_eq!(ids(&[pick("x", true)]), ["folder"]);
        assert!(ids(&[pick("x", true), pick("y", true)]).is_empty());
        assert_eq!(ids(&[pick("a.zip", false), pick("t.txt", false)]), ["two"]);
        assert!(ids(&[]).is_empty());
        assert_eq!(
            shown(&m, &[pick("x", true)])[0].0,
            2,
            "its place in the menu"
        );
    }

    #[test]
    fn requests_read_back_as_written() {
        let r = Request {
            item: "archives".into(),
            paths: vec![
                "/Users/u/a b.zip".into(),
                "/Users/u/tab\there\\and\nline.zip".into(),
            ],
        };
        let text = r.to_text().unwrap();
        assert!(
            text.starts_with("context-menu-rs request 1\nitem\tarchives\npath\t/Users/u/a b.zip\n")
        );
        assert_eq!(Request::from_text(&text), Some(r));
        assert_eq!(
            Request::from_text("context-menu-rs request 2\nitem\tx\n"),
            None
        );
        assert_eq!(
            Request::from_text("context-menu-rs request 1\npath\t/a\n"),
            None,
            "no item"
        );
        assert_eq!(
            Request::from_text("context-menu-rs request 1\nitem\tx\\q\n"),
            None
        );
    }

    #[test]
    fn the_helper_takes_each_request_once() {
        let dir = scratch("take");
        let a = Request {
            item: "archives".into(),
            paths: vec!["/a.zip".into()],
        };
        let b = Request {
            item: "two".into(),
            paths: vec!["/a.zip".into(), "/b.zip".into()],
        };
        leave(&dir, &a).unwrap();
        leave(&dir, &b).unwrap();
        fs::write(dir.join("junk.request"), "not one").unwrap();
        fs::write(dir.join(".half.tmp"), "x").unwrap();
        let now = SystemTime::now();
        let taken = take(&dir, now);
        assert_eq!(taken.len(), 2);
        assert!(taken.contains(&a) && taken.contains(&b));
        let left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, [".half.tmp"], "a file being written stays, for now");
        assert!(take(&dir, now).is_empty());

        // An hour later: the request is not for now, and goes unrun.
        leave(&dir, &a).unwrap();
        let later = now + Duration::from_secs(3600);
        assert!(take(&dir, later).is_empty());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        assert!(take(&scratch("none").join("missing"), now).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_request_runs_its_items_program_with_the_paths() {
        let dir = scratch("run");
        let zip = dir.join("a b.zip");
        let gz = dir.join("c.tar.gz");
        let folder = dir.join("photos");
        fs::write(&zip, "").unwrap();
        fs::write(&gz, "").unwrap();
        fs::create_dir(&folder).unwrap();
        let m = menu();
        let request = |item: &str, paths: &[&PathBuf]| Request {
            item: item.into(),
            paths: paths.iter().map(|p| p.to_path_buf()).collect(),
        };

        let run = command(&m, &request("archives", &[&zip, &gz])).unwrap();
        assert_eq!(run.get_program(), APP);
        let args: Vec<_> = run.get_args().collect();
        assert_eq!(
            args,
            [
                std::ffi::OsStr::new("--open"),
                zip.as_os_str(),
                gz.as_os_str()
            ]
        );
        assert_eq!(run.get_current_dir(), Some(dir.as_path()));

        // Not what the item applies to, not there, not absolute, not the menu's.
        assert!(command(&m, &request("archives", &[&folder])).is_none());
        assert!(command(&m, &request("folder", &[&folder])).is_some());
        assert!(command(&m, &request("archives", &[&dir.join("gone.zip")])).is_none());
        let relative = PathBuf::from("a b.zip");
        assert!(command(&m, &request("archives", &[&relative])).is_none());
        assert!(command(&m, &request("other", &[&zip])).is_none());
    }
}
