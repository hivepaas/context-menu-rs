//! A Windows Explorer extension that shows a context-menu-rs menu in
//! Windows 11's own right-click menu, which takes items only from packaged
//! apps: an `IExplorerCommand`, which an app's package declares.
//!
//! It is built once. An app ships it in its folder under the name of its
//! menu, `context-menu-<menu id>.dll` (`context_menu::windows::native_name`),
//! and its package declares it with the menu's COM class
//! (`context_menu::windows::package_extensions`): when Explorer loads it, it
//! finds its own name and so its menu. Each time Explorer asks what to show
//! it reads that menu's definition, which `context_menu::install` writes in
//! the user's data folder, so a changed menu needs no new package; and the
//! copies of several apps sit side by side, each serving its own menu. See
//! `docs/windows.md`.
//!
//! Explorer's interface is COM: `src/ffi.rs` holds all of that, and the
//! unsafe code with it. What is decided, from which files are chosen to what
//! runs, is plain Rust here, and tested on every system.

// ffi.rs, which uses the rest, is Windows's alone.
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod ffi;

use std::path::{Path, PathBuf};

use context_menu::{Chosen, Item, Menu};

/// The menu a library at `path` serves, from its name:
/// `context-menu-<id>.dll`, the extension in any case.
fn menu_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let (stem, extension) = name.rsplit_once('.')?;
    let id = stem.strip_prefix("context-menu-")?;
    (extension.eq_ignore_ascii_case("dll") && context_menu::valid_id(id)).then(|| id.to_string())
}

/// A file or folder chosen in Explorer.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Picked {
    name: String,
    folder: bool,
    path: PathBuf,
}

impl Picked {
    /// The file or folder at `path`, which Explorer gave.
    fn at(path: PathBuf) -> Option<Picked> {
        Some(Picked {
            name: path.file_name()?.to_str()?.to_string(),
            folder: path.is_dir(),
            path,
        })
    }
}

/// One of the extension's commands, as Explorer holds them: the menu's own,
/// or one of its items by id.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Command {
    Menu,
    Item(String),
}

/// Windows 11's menu takes one entry for the menu: the menu's single item
/// itself, when it has one and no submenu; otherwise a submenu, which
/// Explorer is told of before anything is chosen.
fn single(menu: &Menu) -> Option<&Item> {
    match menu.items.as_slice() {
        [item] if !menu.submenu => Some(item),
        _ => None,
    }
}

/// The item a command runs, if it runs one.
fn item<'m>(menu: &'m Menu, command: &Command) -> Option<&'m Item> {
    match command {
        Command::Menu => single(menu),
        Command::Item(id) => menu.items.iter().find(|i| &i.id == id),
    }
}

/// The command's label.
fn title(menu: &Menu, command: &Command) -> Option<String> {
    match (command, item(menu, command)) {
        (Command::Menu, None) => Some(menu.title.clone()),
        (_, item) => item.map(|i| i.label.clone()),
    }
}

/// The command's icon, if Windows can use it.
fn icon(menu: &Menu, command: &Command) -> Option<String> {
    let icon = match (command, item(menu, command)) {
        (Command::Menu, None) => menu.icon.as_ref(),
        (_, item) => item.and_then(|i| i.icon.as_ref()),
    }?;
    context_menu::windows::is_icon(icon).then(|| icon.clone())
}

/// Whether the menu's entry holds the items.
fn has_items(menu: &Menu) -> bool {
    single(menu).is_none()
}

/// Whether the command shows for the files picked: the menu's when any of
/// its items does.
fn shows(menu: &Menu, command: &Command, picked: &[Picked]) -> bool {
    if picked.is_empty() {
        return false;
    }
    let chosen: Vec<Chosen<'_>> = picked
        .iter()
        .map(|p| Chosen {
            name: &p.name,
            folder: p.folder,
        })
        .collect();
    let accepts = |i: &Item| i.applies_to.accepts(&chosen);
    match (command, item(menu, command)) {
        (Command::Menu, None) => menu.items.iter().any(accepts),
        (_, item) => item.is_some_and(accepts),
    }
}

/// The items' commands, for the menu's submenu.
fn items(menu: &Menu) -> Vec<Command> {
    menu.items
        .iter()
        .map(|i| Command::Item(i.id.clone()))
        .collect()
}

/// What a command runs for the files picked: the program, then its
/// arguments and the paths, in one run, in the first one's folder.
fn action(menu: &Menu, command: &Command, picked: &[Picked]) -> Option<std::process::Command> {
    let item = item(menu, command)?;
    let mut run = std::process::Command::new(&item.program);
    run.args(&item.args).args(picked.iter().map(|p| &p.path));
    if let Some(folder) = picked.first().and_then(|p| p.path.parent()) {
        run.current_dir(folder);
    }
    Some(run)
}

/// The menu `id`, as its definition says now.
fn load(id: &str) -> Result<Menu, context_menu::Error> {
    let data_home = context_menu::windows::data_home()?;
    let path = context_menu::windows::definition_path(id, &data_home);
    let text = std::fs::read_to_string(&path)
        .map_err(|source| context_menu::Error::Io { path, source })?;
    Menu::from_definition(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use context_menu::Count;

    const APP: &str = r"C:\Program Files\Photo Kit\photokit.exe";

    fn menu() -> Menu {
        Menu::new("photo-kit", "Photo Kit")
            .icon(r"C:\Program Files\Photo Kit\photokit.exe,0")
            .item(
                Item::new("resize", "Resize", APP)
                    .args(["--resize"])
                    .icon("image-x-generic")
                    .extensions(["jpg", "tar.gz"]),
            )
            .item(
                Item::new("two", "Compare", APP)
                    .icon(r"C:\Program Files\Photo Kit\compare.ico")
                    .count(Count::Many),
            )
            .item(
                Item::new("pack", "Pack", APP)
                    .folders_only()
                    .count(Count::One),
            )
    }

    fn pick(name: &str, folder: bool) -> Picked {
        Picked {
            name: name.into(),
            folder,
            path: PathBuf::from(r"C:\Users\u\Pictures").join(name),
        }
    }

    #[test]
    fn the_name_says_the_menu() {
        let id = |p: &str| menu_id(Path::new(p));
        let installed = Path::new("Photo Kit").join("context-menu-photo-kit.dll");
        assert_eq!(menu_id(&installed), Some("photo-kit".into()));
        assert_eq!(id("context-menu-photo-kit.DLL"), Some("photo-kit".into()));
        assert_eq!(id("context_menu_windows.dll"), None, "not renamed");
        assert_eq!(id("context-menu-.dll"), None);
        assert_eq!(id("context-menu-Photo.dll"), None);
        assert_eq!(id("context-menu-photo-kit.exe"), None);
    }

    #[test]
    fn the_menu_shows_when_an_item_does() {
        let m = menu();
        let jpg = [pick("a.JPG", false)];
        assert!(has_items(&m));
        assert_eq!(title(&m, &Command::Menu).as_deref(), Some("Photo Kit"));
        assert!(shows(&m, &Command::Menu, &jpg));
        assert!(!shows(&m, &Command::Menu, &[pick("a.txt", false)]));
        assert!(!shows(&m, &Command::Menu, &[]));

        let shown: Vec<Command> = items(&m)
            .into_iter()
            .filter(|c| shows(&m, c, &jpg))
            .collect();
        assert_eq!(shown, [Command::Item("resize".into())]);
        let two = [pick("a.jpg", false), pick("b.png", false)];
        let shown: Vec<Command> = items(&m)
            .into_iter()
            .filter(|c| shows(&m, c, &two))
            .collect();
        assert_eq!(shown, [Command::Item("two".into())]);
        let folders = [pick("x", true), pick("y", true)];
        assert!(!shows(&m, &Command::Menu, &folders), "pack takes one");
    }

    #[test]
    fn icons_are_windows_own() {
        let m = menu();
        assert_eq!(
            icon(&m, &Command::Menu).as_deref(),
            Some(r"C:\Program Files\Photo Kit\photokit.exe,0")
        );
        assert_eq!(icon(&m, &Command::Item("resize".into())), None);
        assert_eq!(
            icon(&m, &Command::Item("two".into())).as_deref(),
            Some(r"C:\Program Files\Photo Kit\compare.ico")
        );
    }

    #[test]
    fn a_single_item_is_the_entry() {
        let m = Menu::new("photo-kit", "Photo Kit")
            .item(Item::new("resize", "Resize", APP).extensions(["jpg"]));
        assert!(!has_items(&m));
        assert_eq!(title(&m, &Command::Menu).as_deref(), Some("Resize"));
        assert!(shows(&m, &Command::Menu, &[pick("a.jpg", false)]));
        assert!(!shows(&m, &Command::Menu, &[pick("a.png", false)]));
        // Asked for, a submenu it is.
        assert!(has_items(&m.clone().submenu(true)));
    }

    #[test]
    fn an_item_runs_once_for_all() {
        let m = menu();
        let picked = [pick("a b.jpg", false), pick("c.jpg", false)];
        let run = action(&m, &Command::Item("two".into()), &picked).unwrap();
        assert_eq!(run.get_program(), APP);
        let args: Vec<_> = run.get_args().collect();
        let paths: Vec<_> = picked.iter().map(|p| p.path.as_os_str()).collect();
        assert_eq!(args, paths);
        assert_eq!(
            run.get_current_dir(),
            Some(Path::new(r"C:\Users\u\Pictures"))
        );
        assert!(action(&m, &Command::Menu, &picked).is_none());
        assert!(action(&m, &Command::Item("gone".into()), &picked).is_none());
    }
}
