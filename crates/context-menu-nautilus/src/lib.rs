//! A Nautilus extension that shows a context-menu-rs menu natively: in the
//! menu itself, only for the files it applies to, with no nautilus-python.
//!
//! It is built once. An app's package installs it in Nautilus's extension
//! folder under the name of its menu, `libcontext-menu-<menu id>.so`
//! (`context_menu::linux::native_name`): when Nautilus loads it, it finds
//! its own name and so its menu. Each time Nautilus asks for menu items it
//! reads that menu's definition, which `context_menu::install` writes in the
//! user's data folder, so a changed menu needs no new extension; and the
//! copies of several apps sit side by side, each serving its own menu. See
//! `docs/nautilus.md`.
//!
//! Nautilus's interface is C: `src/ffi.rs` holds all of that, and the
//! unsafe code with it. What is decided, from which files are chosen to what
//! runs, is plain Rust here.

#![cfg(target_os = "linux")]
// ffi.rs, which uses the rest, is left out of the unit tests: they would
// need Nautilus's symbols to link.
#![cfg_attr(test, allow(dead_code))]

#[cfg(not(test))]
mod ffi;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use context_menu::{Chosen, Menu};

/// The menu a library at `path` serves, from its name:
/// `libcontext-menu-<id>.so`.
fn menu_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_prefix("libcontext-menu-")?.strip_suffix(".so")?;
    context_menu::valid_id(id).then(|| id.to_string())
}

/// A file or folder Nautilus asks about.
struct Picked {
    name: String,
    folder: bool,
    path: PathBuf,
}

/// What an entry runs: the program, its arguments, then the paths.
struct Action {
    program: PathBuf,
    args: Vec<String>,
    paths: Vec<PathBuf>,
}

/// An entry to show: its name for Nautilus, its label and icon, and what it
/// runs.
struct Entry {
    name: String,
    label: String,
    icon: Option<String>,
    action: Action,
}

/// The submenu holding the entries, when the menu has one: its name for
/// Nautilus, its title and icon.
struct Submenu {
    name: String,
    title: String,
    icon: Option<String>,
}

/// What `menu` shows for the files picked.
fn entries(menu: &Menu, picked: &[Picked]) -> (Option<Submenu>, Vec<Entry>) {
    let chosen: Vec<Chosen<'_>> = picked
        .iter()
        .map(|p| Chosen {
            name: &p.name,
            folder: p.folder,
        })
        .collect();
    let list = menu
        .items
        .iter()
        .filter(|i| i.applies_to.accepts(&chosen))
        .map(|i| Entry {
            name: format!("context-menu-rs::{}::{}", menu.id, i.id),
            label: i.label.clone(),
            icon: i.icon.clone(),
            action: Action {
                program: i.program.clone(),
                args: i.args.clone(),
                paths: picked.iter().map(|p| p.path.clone()).collect(),
            },
        })
        .collect();
    let submenu = menu.submenu.then(|| Submenu {
        name: format!("context-menu-rs::{}", menu.id),
        title: menu.title.clone(),
        icon: menu.icon.clone(),
    });
    (submenu, list)
}

/// The menu `id`, as its definition says now.
fn load(id: &str) -> Result<Menu, context_menu::Error> {
    let data_home = context_menu::linux::data_home()?;
    let path = context_menu::linux::definition_path(id, &data_home);
    let text = std::fs::read_to_string(&path)
        .map_err(|source| context_menu::Error::Io { path, source })?;
    Menu::from_definition(&text)
}

/// Says why the menu cannot be shown, once: Nautilus asks at every click.
fn warn_once(why: &dyn std::fmt::Display) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!("context-menu-nautilus: {why}");
    }
}

/// Runs an entry's program without waiting for it, and reaps it when it
/// ends, so it leaves no zombie in Nautilus.
fn run(action: &Action) {
    let started = std::process::Command::new(&action.program)
        .args(&action.args)
        .args(&action.paths)
        .spawn();
    match started {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("context-menu-nautilus: {}: {e}", action.program.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use context_menu::{Count, Item};

    #[test]
    fn the_name_says_the_menu() {
        let id = |p: &str| menu_id(Path::new(p));
        assert_eq!(
            id("/usr/lib/x86_64-linux-gnu/nautilus/extensions-4/libcontext-menu-photo-kit.so"),
            Some("photo-kit".into())
        );
        assert_eq!(id("libcontext_menu_nautilus.so"), None, "not renamed");
        assert_eq!(id("libcontext-menu-.so"), None);
        assert_eq!(id("libcontext-menu-Photo.so"), None);
        assert_eq!(id("libcontext-menu-x.so.1"), None);
    }

    #[test]
    fn entries_are_the_items_for_the_files_picked() {
        let menu = Menu::new("photo-kit", "Photo Kit")
            .submenu(true)
            .item(
                Item::new("resize", "Resize", "/usr/bin/photokit")
                    .args(["--resize"])
                    .extensions(["jpg"]),
            )
            .item(Item::new("two", "Compare", "/usr/bin/photokit").count(Count::Many));
        let pick = |name: &str| Picked {
            name: name.into(),
            folder: false,
            path: PathBuf::from("/home/u").join(name),
        };
        let (submenu, list) = entries(&menu, &[pick("a.JPG")]);
        assert_eq!(
            submenu.map(|s| s.name),
            Some("context-menu-rs::photo-kit".into())
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "context-menu-rs::photo-kit::resize");
        assert_eq!(list[0].action.args, ["--resize"]);
        assert_eq!(list[0].action.paths, [PathBuf::from("/home/u/a.JPG")]);

        let (_, list) = entries(&menu, &[pick("a.jpg"), pick("b.png")]);
        let labels: Vec<_> = list.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Compare"]);
    }
}
