//! Nemo (Cinnamon): an action per item in `nemo/actions`. Nemo has no way
//! for an action to put itself in a submenu; its users group actions in
//! Nemo's own Actions layout.

use std::fmt::Write as _;

use super::quote::{key_value, shell};
use super::{Dirs, File, mark, named};
use crate::{Count, Error, Files, Item, Menu};

fn dir(dirs: &Dirs) -> std::path::PathBuf {
    dirs.data_home.join("nemo/actions")
}

pub(super) fn files(menu: &Menu, dirs: &Dirs, out: &mut Vec<File>, notes: &mut Vec<String>) {
    for item in &menu.items {
        out.push(File {
            path: dir(dirs).join(format!("{}.{}.nemo_action", menu.id, item.id)),
            text: action(menu, item),
            executable: false,
        });
    }
    if menu.submenu {
        notes.push(
            "Nemo: the items are not grouped in a submenu; Nemo's Actions layout can group them."
                .into(),
        );
    }
}

pub(super) fn existing(id: &str, dirs: &Dirs) -> Result<Vec<std::path::PathBuf>, Error> {
    named(&dir(dirs), id, ".nemo_action")
}

fn action(menu: &Menu, item: &Item) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# {}", mark(&menu.id));
    s.push_str("[Nemo Action]\n");
    // Nemo fills in %-tokens in the name and the command.
    let _ = writeln!(s, "Name={}", key_value(&item.label.replace('%', "%%")));
    // Nemo splits the command as a shell would, and quotes the paths itself.
    let mut exec = shell(&item.program.to_string_lossy());
    for arg in &item.args {
        exec.push(' ');
        exec.push_str(&shell(arg));
    }
    let exec = exec.replace('%', "%%") + " %F";
    let _ = writeln!(s, "Exec={}", key_value(&exec));
    // Only a theme's icon names: no paths.
    if let Some(icon) = item.icon.as_ref().filter(|i| !i.starts_with('/')) {
        let _ = writeln!(s, "Icon-Name={}", key_value(icon));
    }
    let selection = match item.applies_to.count {
        Count::One => "s",
        Count::Many => "m",
        Count::Any => "notnone",
    };
    let _ = writeln!(s, "Selection={selection}");
    let _ = writeln!(s, "Extensions={}", extensions(item));
    // Paths are passed, so only local folders.
    s.push_str("UriScheme=file\n");
    s
}

/// Nemo's `Extensions`: its words, or the endings it matches names
/// against. It matches the name's end alone, so each gets its dot.
fn extensions(item: &Item) -> String {
    let folders = item.applies_to.folders;
    let mut words: Vec<String> = match &item.applies_to.files {
        Files::Any if folders => return "any;".into(),
        Files::Any => return "nodirs;".into(),
        Files::None => Vec::new(),
        Files::Extensions(list) => list.iter().map(|e| format!(".{e}")).collect(),
    };
    if folders {
        words.push("dir".into());
    }
    words.iter().map(|w| format!("{};", key_value(w))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_an_action() {
        let menu = Menu::new("packwing", "packwing").submenu(true).item(
            Item::new("extract", "Extract 100%", "/opt/Pack Wing/packwing")
                .args(["--to", "it's here"])
                .icon("package-x-generic")
                .extensions(["zip", "tar.gz"])
                .folders(true)
                .count(Count::Many),
        );
        let mut out = Vec::new();
        let mut notes = Vec::new();
        files(
            &menu,
            &crate::linux::testing::dirs("nemo"),
            &mut out,
            &mut notes,
        );
        assert_eq!(
            out[0].text,
            "# Made by context-menu-rs for the menu \"packwing\". It is rewritten on install.\n\
             [Nemo Action]\n\
             Name=Extract 100%%\n\
             Exec='/opt/Pack Wing/packwing' '--to' 'it'\\\\''s here' %F\n\
             Icon-Name=package-x-generic\n\
             Selection=m\n\
             Extensions=.zip;.tar.gz;dir;\n\
             UriScheme=file\n"
        );
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn extensions_say_what_items_apply_to() {
        let item = |files: Files, folders| {
            let mut i = Item::new("x", "X", "/bin/x");
            i.applies_to.files = files;
            i.applies_to.folders = folders;
            extensions(&i)
        };
        assert_eq!(item(Files::Any, true), "any;");
        assert_eq!(item(Files::Any, false), "nodirs;");
        assert_eq!(item(Files::None, true), "dir;");
        assert_eq!(item(Files::Extensions(vec!["7z".into()]), false), ".7z;");
    }
}
