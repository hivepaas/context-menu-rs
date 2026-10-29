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
//! every system, and written on Windows alone. The names the native
//! extension and an app's package use for Windows 11's own menu are here
//! too, for the extension and for an app's build on any system.

// The plan is used by the registry's code, on Windows, and by tests.
#![cfg_attr(not(windows), allow(dead_code))]

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

/// Whether Windows can use `icon`: a path to an `.ico`, or to an `.exe` or
/// `.dll` and an index (`app.exe,0`). An icon theme's name, for Linux, is
/// not one.
pub fn is_icon(icon: &str) -> bool {
    let file = icon.rsplit_once(',').map_or(icon, |(f, _)| f);
    let lower = file.to_ascii_lowercase();
    let is_path = Path::new(file).is_absolute() || file.contains('\\');
    is_path && [".ico", ".exe", ".dll"].iter().any(|e| lower.ends_with(e))
}

fn icon(icon: Option<&String>) -> Option<&str> {
    icon.map(String::as_str).filter(|i| is_icon(i))
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
        // Explorer shows one verb of a name, an extension's over `*`'s, so
        // a submenu under an extension holds the items for any file too.
        let extension = class.starts_with("SystemFileAssociations\\");
        let items: Vec<(usize, &Item)> = menu
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                let of = classes(i);
                of.contains(class) || (menu.submenu && extension && of.iter().any(|c| c == "*"))
            })
            .collect();
        if menu.submenu {
            let parent = format!("{class}\\shell\\{}", menu.id);
            // Windows reads MultiSelectModel for the submenu's own verb, not
            // its items', and shows the submenu even with nothing in it: it
            // takes its items' count and condition when they all have one.
            let model = if items.iter().all(|(_, i)| i.applies_to.count == Count::One) {
                "Single"
            } else {
                "Player"
            };
            let mut values = vec![
                ("MUIVerb".to_string(), menu.title.clone()),
                ("SubCommands".to_string(), String::new()),
                ("MultiSelectModel".to_string(), model.to_string()),
            ];
            if let Some(icon) = icon(menu.icon.as_ref()) {
                values.push(("Icon".into(), icon.into()));
            }
            let filters: Option<Vec<String>> =
                items.iter().map(|(_, i)| applies_to(i, class)).collect();
            if let Some(mut filters) = filters {
                filters.sort();
                filters.dedup();
                values.push(("AppliesTo".into(), filters.join(" OR ")));
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
            for &(n, item) in &items {
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
            for &(n, item) in &items {
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

/// The name the native extension, `context-menu-windows`, has when it
/// serves the menu `id`: `context-menu-<id>.dll`. See `docs/windows.md`.
pub fn native_name(id: &str) -> String {
    format!("context-menu-{id}.dll")
}

/// The COM class of the native extension serving the menu `id`, as an app's
/// package declares it: a GUID made from the id alone (RFC 9562's version
/// 5), so the extension knows it too, in upper case with no braces.
pub fn clsid(id: &str) -> String {
    // context-menu-rs's own namespace, ae650857-d795-49b3-ad8d-995659ed5e7b.
    const NAMESPACE: [u8; 16] = [
        0xae, 0x65, 0x08, 0x57, 0xd7, 0x95, 0x49, 0xb3, 0xad, 0x8d, 0x99, 0x56, 0x59, 0xed, 0x5e,
        0x7b,
    ];
    let mut name = NAMESPACE.to_vec();
    name.extend_from_slice(id.as_bytes());
    let hash = sha1(&name);
    let mut out = String::with_capacity(36);
    for (i, b) in hash.iter().take(16).enumerate() {
        let b = match i {
            6 => (b & 0x0f) | 0x50,
            8 => (b & 0x3f) | 0x80,
            _ => *b,
        };
        if [4, 6, 8, 10].contains(&i) {
            out.push('-');
        }
        out.push_str(&format!("{b:02X}"));
    }
    out
}

/// What an app's package manifest declares for the menu `id`, inside its
/// `<Application>`'s `<Extensions>`: Windows 11's menu, for any file and
/// any folder, served by the native extension under its name, in the
/// package's folder. The manifest declares the namespaces `desktop4`,
/// `desktop5` and `com`. See `docs/windows.md`.
pub fn package_extensions(id: &str) -> String {
    let clsid = clsid(id);
    let name = native_name(id);
    let verb: String = id.chars().filter(char::is_ascii_alphanumeric).collect();
    format!(
        r#"<desktop4:Extension Category="windows.fileExplorerContextMenus">
  <desktop4:FileExplorerContextMenus>
    <desktop5:ItemType Type="*">
      <desktop5:Verb Id="ContextMenuRs{verb}" Clsid="{clsid}" />
    </desktop5:ItemType>
    <desktop5:ItemType Type="Directory">
      <desktop5:Verb Id="ContextMenuRs{verb}" Clsid="{clsid}" />
    </desktop5:ItemType>
  </desktop4:FileExplorerContextMenus>
</desktop4:Extension>
<com:Extension Category="windows.comServer">
  <com:ComServer>
    <com:SurrogateServer DisplayName="{id}">
      <com:Class Id="{clsid}" Path="{name}" ThreadingModel="STA" />
    </com:SurrogateServer>
  </com:ComServer>
</com:Extension>
"#
    )
}

/// SHA-1, for [`clsid`] alone.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..20 => ((b & c) | (!b & d), 0x5A82_7999),
                20..40 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..60 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (chunk, x) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *chunk = x.to_be_bytes();
    }
    out
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
    fn sha1_is_sha1() {
        let hex = |d: &[u8]| {
            sha1(d)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert_eq!(hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(&[b'a'; 1000]),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
    }

    #[test]
    fn a_menu_has_its_own_class() {
        // As Python's uuid.uuid5 makes them.
        assert_eq!(clsid("packwing"), "AC1A75AA-BC84-59DB-BD47-0E295821318E");
        assert_eq!(
            clsid("context-menu-demo"),
            "95BB8076-AA57-58EE-84F1-C022DADC184D"
        );
        let xml = package_extensions("photo-kit");
        assert!(xml.contains(r#"Clsid="E09045CE-CCB6-5DBD-8BE4-F08B7DEB3BFC""#));
        assert!(xml.contains(r#"Path="context-menu-photo-kit.dll""#));
        assert!(xml.contains(r#"Id="ContextMenuRsphotokit""#));
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
                r"SystemFileAssociations\.zip\shell\packwing\shell\01.compress",
                r"SystemFileAssociations\.zip\shell\packwing\shell\01.compress\command",
                r"SystemFileAssociations\.gz\shell\packwing",
                r"SystemFileAssociations\.gz\shell\packwing\shell",
                r"SystemFileAssociations\.gz\shell\packwing\shell\00.extract",
                r"SystemFileAssociations\.gz\shell\packwing\shell\00.extract\command",
                r"SystemFileAssociations\.gz\shell\packwing\shell\01.compress",
                r"SystemFileAssociations\.gz\shell\packwing\shell\01.compress\command",
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
        // Under .gz, only names ending in .tar.gz; the submenu shows for
        // every .gz, for "Compress".
        assert!(keys[8].values.contains(&(
            "AppliesTo".to_string(),
            r#"System.FileName:~>".tar.gz""#.to_string()
        )));
        assert!(!keys[6].values.iter().any(|(name, _)| name == "AppliesTo"));
    }

    #[test]
    fn a_submenu_takes_what_all_its_items_have() {
        let menu = Menu::new("packwing", "Pack Wing").submenu(true).item(
            Item::new("extract", "Extract here", APP)
                .extensions(["zip", "tar.gz"])
                .count(Count::One),
        );
        let keys = plan(&menu);
        let parent = |path: &str| {
            keys.iter()
                .find(|k| k.path == path)
                .map(|k| k.values.clone())
                .unwrap_or_default()
        };
        let gz = parent(r"SystemFileAssociations\.gz\shell\packwing");
        assert!(gz.contains(&("MultiSelectModel".to_string(), "Single".to_string())));
        assert!(gz.contains(&(
            "AppliesTo".to_string(),
            r#"System.FileName:~>".tar.gz""#.to_string()
        )));
        let zip = parent(r"SystemFileAssociations\.zip\shell\packwing");
        assert!(zip.contains(&("MultiSelectModel".to_string(), "Single".to_string())));
        assert!(!zip.iter().any(|(name, _)| name == "AppliesTo"));
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
