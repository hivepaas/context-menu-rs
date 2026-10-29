//! Windows: the classic right-click menu, from verbs in the user's part of
//! the registry (`HKEY_CURRENT_USER\Software\Classes`), with no rights
//! beyond the user's. It is the menu itself on Windows 10, and on Windows
//! 11 the one under "Show more options". See `docs/windows.md`.
//!
//! An item is a verb under each kind of thing it applies to: `*` for any
//! file, `SystemFileAssociations\.<extension>` for some, `Directory` for
//! folders. Windows runs a verb's command once for each file chosen.
//!
//! What goes in the registry is planned here in plain Rust, the same on
//! every system, and written on Windows alone.

use std::path::{Path, PathBuf};

use crate::{Count, Files, Item, Menu};

/// The value every verb of the menu's carries, its id: what uninstalling
/// looks for.
const MARK: &str = "ContextMenuRs";

/// A key the menu writes, under the user's classes, with its values, all
/// strings; the name `""` is the key's default value.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key {
    pub path: String,
    pub values: Vec<(String, String)>,
}

/// Where an item's verb goes: the class keys of what it applies to.
fn classes(item: &Item) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match &item.applies_to.files {
        Files::None => {}
        Files::Any => out.push("*".into()),
        Files::Extensions(list) => {
            for ext in list {
                // Windows knows a file's last extension alone: `tar.gz`'s
                // class is `.gz`, and AppliesTo narrows it.
                let last = ext.rsplit('.').next().unwrap_or(ext);
                let class = format!("SystemFileAssociations\\.{last}");
                if !out.contains(&class) {
                    out.push(class);
                }
            }
        }
    }
    if item.applies_to.folders {
        out.push("Directory".into());
    }
    out
}

/// For an item under an extension's class, the condition narrowing it to
/// the longer extensions that end in it: `tar.gz` under `.gz`. None when
/// the class's own extension is one of the item's.
fn applies_to(item: &Item, class: &str) -> Option<String> {
    let Files::Extensions(list) = &item.applies_to.files else {
        return None;
    };
    let ext = class.strip_prefix("SystemFileAssociations\\.")?;
    if list.iter().any(|e| e == ext) {
        return None;
    }
    let longer: Vec<String> = list
        .iter()
        .filter(|e| e.rsplit('.').next() == Some(ext))
        .map(|e| format!("System.FileName:~>\".{e}\""))
        .collect();
    (!longer.is_empty()).then(|| longer.join(" OR "))
}

/// An icon Windows can use: a path to an `.ico`, or to an `.exe` or `.dll`
/// and an index (`app.exe,0`). An icon theme's name, for Linux, is not one.
fn icon(icon: Option<&String>) -> Option<&str> {
    let icon = icon?;
    let file = icon.rsplit_once(',').map_or(icon.as_str(), |(f, _)| f);
    let lower = file.to_ascii_lowercase();
    let is_path = Path::new(file).is_absolute() || file.contains('\\');
    (is_path && [".ico", ".exe", ".dll"].iter().any(|e| lower.ends_with(e)))
        .then_some(icon.as_str())
}

/// An argument quoted as `CommandLineToArgvW` reads it back: in quotes,
/// a quote as `\"`, and the backslashes before a quote, or the closing
/// one, doubled.
fn quote(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
            continue;
        }
        if c == '"' {
            out.push_str(&"\\".repeat(backslashes * 2 + 1));
        } else {
            out.push_str(&"\\".repeat(backslashes));
        }
        backslashes = 0;
        out.push(c);
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// The item's command: the program, its arguments, and the file chosen.
fn command(item: &Item) -> String {
    let mut s = quote(&item.program.to_string_lossy());
    for arg in &item.args {
        s.push(' ');
        s.push_str(&quote(arg));
    }
    s.push_str(" \"%1\"");
    s
}

/// The values of an item's verb under `class`.
fn verb(item: &Item, class: &str) -> Vec<(String, String)> {
    let mut v = vec![("MUIVerb".to_string(), item.label.clone())];
    if let Some(icon) = icon(item.icon.as_ref()) {
        v.push(("Icon".into(), icon.into()));
    }
    let model = match item.applies_to.count {
        Count::One => "Single",
        // "Player": shown for any number chosen, past Windows's 15.
        Count::Many | Count::Any => "Player",
    };
    v.push(("MultiSelectModel".into(), model.into()));
    if let Some(filter) = applies_to(item, class) {
        v.push(("AppliesTo".into(), filter));
    }
    v
}

/// The keys `menu` writes, each verb before its command. The verbs are
/// named with their place in the menu, which Windows orders them by.
pub(crate) fn plan(menu: &Menu) -> Vec<Key> {
    let mut all_classes: Vec<String> = Vec::new();
    for item in &menu.items {
        for c in classes(item) {
            if !all_classes.contains(&c) {
                all_classes.push(c);
            }
        }
    }
    let mark = (MARK.to_string(), menu.id.clone());
    let mut keys = Vec::new();
    for class in &all_classes {
        let items = menu
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| classes(i).contains(class));
        if menu.submenu {
            let parent = format!("{class}\\shell\\{}", menu.id);
            let mut values = vec![
                ("MUIVerb".to_string(), menu.title.clone()),
                ("SubCommands".to_string(), String::new()),
                ("MultiSelectModel".to_string(), "Player".to_string()),
            ];
            if let Some(icon) = icon(menu.icon.as_ref()) {
                values.push(("Icon".into(), icon.into()));
            }
            values.push(mark.clone());
            keys.push(Key {
                path: parent.clone(),
                values,
            });
            // The key the items' verbs are in, with no values of its own.
            keys.push(Key {
                path: format!("{parent}\\shell"),
                values: Vec::new(),
            });
            for (n, item) in items {
                let path = format!("{parent}\\shell\\{n:02}.{}", item.id);
                keys.push(Key {
                    path: path.clone(),
                    values: verb(item, class),
                });
                keys.push(Key {
                    path: format!("{path}\\command"),
                    values: vec![(String::new(), command(item))],
                });
            }
        } else {
            for (n, item) in items {
                let path = format!("{class}\\shell\\{}.{n:02}.{}", menu.id, item.id);
                let mut values = verb(item, class);
                values.push(mark.clone());
                keys.push(Key {
                    path: path.clone(),
                    values,
                });
                keys.push(Key {
                    path: format!("{path}\\command"),
                    values: vec![(String::new(), command(item))],
                });
            }
        }
    }
    keys
}

/// The user's data folder, `%LOCALAPPDATA%`: where the menu's definition
/// goes, for the native extension.
pub fn data_home() -> Result<PathBuf, crate::Error> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or(crate::Error::NoHome)
}

/// Where the definition of the menu `id` goes in `data_home`.
pub fn definition_path(id: &str, data_home: &Path) -> PathBuf {
    data_home.join("context-menu-rs").join(format!("{id}.menu"))
}

#[cfg(windows)]
mod registry;
#[cfg(windows)]
pub use registry::{Root, install, is_current, uninstall};

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = r"C:\Program Files\Pack Wing\packwing.exe";

    fn menu(submenu: bool) -> Menu {
        Menu::new("packwing", "Pack Wing")
            .submenu(submenu)
            .icon(r"C:\Program Files\Pack Wing\packwing.exe,0")
            .item(
                Item::new("extract", "Extract here", APP)
                    .args(["--extract", r#"say "hi""#])
                    .icon("package-x-generic")
                    .extensions(["zip", "tar.gz"])
                    .count(Count::One),
            )
            .item(Item::new("compress", "Compress", APP).folders(true))
    }

    #[test]
    fn arguments_read_back_as_written() {
        assert_eq!(quote("plain"), r#""plain""#);
        assert_eq!(quote("two words"), r#""two words""#);
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(r"C:\dir\"), r#""C:\dir\\""#);
        assert_eq!(quote(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(quote(""), r#""""#);
    }

    #[test]
    fn icons_are_paths_on_windows() {
        let i = |s: &str| icon(Some(&s.to_string())).map(str::to_string);
        assert_eq!(i(r"C:\app\app.exe,0").as_deref(), Some(r"C:\app\app.exe,0"));
        assert_eq!(i(r"C:\app\icon.ico").as_deref(), Some(r"C:\app\icon.ico"));
        assert_eq!(i("package-x-generic"), None);
        assert_eq!(i("/usr/share/icons/x.png"), None);
    }

    #[test]
    fn a_submenu_per_kind_of_thing() {
        let keys = plan(&menu(true));
        let paths: Vec<&str> = keys.iter().map(|k| k.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                r"SystemFileAssociations\.zip\shell\packwing",
                r"SystemFileAssociations\.zip\shell\packwing\shell",
                r"SystemFileAssociations\.zip\shell\packwing\shell\00.extract",
                r"SystemFileAssociations\.zip\shell\packwing\shell\00.extract\command",
                r"SystemFileAssociations\.gz\shell\packwing",
                r"SystemFileAssociations\.gz\shell\packwing\shell",
                r"SystemFileAssociations\.gz\shell\packwing\shell\00.extract",
                r"SystemFileAssociations\.gz\shell\packwing\shell\00.extract\command",
                r"*\shell\packwing",
                r"*\shell\packwing\shell",
                r"*\shell\packwing\shell\01.compress",
                r"*\shell\packwing\shell\01.compress\command",
                r"Directory\shell\packwing",
                r"Directory\shell\packwing\shell",
                r"Directory\shell\packwing\shell\01.compress",
                r"Directory\shell\packwing\shell\01.compress\command",
            ]
        );
        assert_eq!(
            keys[0].values,
            [
                ("MUIVerb".to_string(), "Pack Wing".to_string()),
                ("SubCommands".into(), String::new()),
                ("MultiSelectModel".into(), "Player".into()),
                (
                    "Icon".into(),
                    r"C:\Program Files\Pack Wing\packwing.exe,0".into()
                ),
                ("ContextMenuRs".into(), "packwing".into()),
            ]
        );
        assert!(keys[1].values.is_empty());
        // The theme icon is Linux's: left out.
        assert_eq!(
            keys[2].values,
            [
                ("MUIVerb".to_string(), "Extract here".to_string()),
                ("MultiSelectModel".into(), "Single".into()),
            ]
        );
        assert_eq!(
            keys[3].values,
            [(
                String::new(),
                r#""C:\Program Files\Pack Wing\packwing.exe" "--extract" "say \"hi\"" "%1""#
                    .to_string()
            )]
        );
        // Under .gz, only names ending in .tar.gz.
        assert!(keys[6].values.contains(&(
            "AppliesTo".to_string(),
            r#"System.FileName:~>".tar.gz""#.to_string()
        )));
    }

    #[test]
    fn without_a_submenu_each_item_is_a_verb() {
        let keys = plan(&menu(false));
        assert_eq!(
            keys[0].path,
            r"SystemFileAssociations\.zip\shell\packwing.00.extract"
        );
        assert!(
            keys[0]
                .values
                .contains(&("ContextMenuRs".to_string(), "packwing".to_string()))
        );
        assert_eq!(
            keys[1].path,
            r"SystemFileAssociations\.zip\shell\packwing.00.extract\command"
        );
        assert_eq!(keys.len(), 8);
    }
}
