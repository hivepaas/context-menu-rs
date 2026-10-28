//! Dolphin, and KDE's other file views: a service menu per item in
//! `kio/servicemenus`, as KDE Frameworks 5.85 and later read them. An item a
//! file of its own, since what a menu shows for is set per file; those with
//! the same `X-KDE-Submenu` share the submenu.

use std::fmt::Write as _;

use super::quote::{exec_arg, key_value};
use super::{Dirs, File, mark, mime, remove_named};
use crate::{Count, Error, Files, Item, Menu, Report};

fn dir(dirs: &Dirs) -> std::path::PathBuf {
    dirs.data_home.join("kio/servicemenus")
}

pub(super) fn files(menu: &Menu, dirs: &Dirs, out: &mut Vec<File>, notes: &mut Vec<String>) {
    for item in &menu.items {
        let Some(types) = mime_types(item, dirs, notes) else {
            continue;
        };
        out.push(File {
            path: dir(dirs).join(format!("{}.{}.desktop", menu.id, item.id)),
            text: entry(menu, item, &types),
            // KIO reads service menus in the user's folder only when they
            // are executable.
            executable: true,
        });
    }
}

pub(super) fn remove(id: &str, dirs: &Dirs, report: &mut Report) -> Result<(), Error> {
    remove_named(&dir(dirs), id, ".desktop", report)
}

/// The MIME types `item` shows for, or none when it would show for nothing
/// here (a note says why).
fn mime_types(item: &Item, dirs: &Dirs, notes: &mut Vec<String>) -> Option<Vec<String>> {
    let mut types = Vec::new();
    match &item.applies_to.files {
        Files::None => {}
        // Every file's type is a kind of this one.
        Files::Any => types.push("application/octet-stream".to_string()),
        Files::Extensions(list) => {
            for ext in list {
                let found = mime::types_of(ext, &dirs.data_dirs);
                if found.is_empty() {
                    notes.push(format!(
                        "Dolphin: no MIME type is known here for .{ext}, so {:?} does not show \
                         for those files.",
                        item.label
                    ));
                }
                for t in found {
                    if !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
        }
    }
    if item.applies_to.folders {
        types.push("inode/directory".to_string());
    }
    if types.is_empty() {
        notes.push(format!("Dolphin: {:?} is left out.", item.label));
        return None;
    }
    Some(types)
}

fn entry(menu: &Menu, item: &Item, types: &[String]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# {}", mark(&menu.id));
    s.push_str("[Desktop Entry]\nType=Service\n");
    // KIO goes by the action's name; the specification asks for one here too.
    let _ = writeln!(s, "Name={}", key_value(&item.label));
    let _ = writeln!(s, "MimeType={};", types.join(";"));
    // Paths are passed, so only local files.
    s.push_str("X-KDE-Protocols=file\n");
    // In the menu itself, not in its Actions submenu.
    s.push_str("X-KDE-Priority=TopLevel\n");
    if menu.submenu {
        let _ = writeln!(s, "X-KDE-Submenu={}", key_value(&menu.title));
        if let Some(icon) = &menu.icon {
            let _ = writeln!(s, "Icon={}", key_value(icon));
        }
    }
    match item.applies_to.count {
        Count::One => s.push_str("X-KDE-MaxNumberOfUrls=1\n"),
        Count::Many => s.push_str("X-KDE-MinNumberOfUrls=2\n"),
        Count::Any => {}
    }
    s.push_str("Actions=run;\n\n[Desktop Action run]\n");
    let _ = writeln!(s, "Name={}", key_value(&item.label));
    if let Some(icon) = &item.icon {
        let _ = writeln!(s, "Icon={}", key_value(icon));
    }
    let mut exec = exec_arg(&item.program.to_string_lossy());
    for arg in &item.args {
        exec.push(' ');
        exec.push_str(&exec_arg(arg));
    }
    exec.push_str(" %F");
    let _ = writeln!(s, "Exec={}", key_value(&exec));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::testing::dirs;

    #[test]
    fn an_item_is_a_service_menu() {
        let d = dirs("dolphin", false);
        let menu = Menu::new("packwing", "packwing")
            .submenu(true)
            .icon("package-x-generic")
            .item(
                Item::new("extract", "Extract here", "/opt/Pack Wing/packwing")
                    .args(["--extract", "100%"])
                    .extensions(["zip", "7z", "rar"])
                    .folders(true)
                    .count(Count::One),
            );
        let mut out = Vec::new();
        let mut notes = Vec::new();
        files(&menu, &d, &mut out, &mut notes);
        assert_eq!(
            out[0].text,
            "# Made by context-menu-rs for the menu \"packwing\". It is rewritten on install.\n\
             [Desktop Entry]\n\
             Type=Service\n\
             Name=Extract here\n\
             MimeType=application/zip;application/x-7z-compressed;inode/directory;\n\
             X-KDE-Protocols=file\n\
             X-KDE-Priority=TopLevel\n\
             X-KDE-Submenu=packwing\n\
             Icon=package-x-generic\n\
             X-KDE-MaxNumberOfUrls=1\n\
             Actions=run;\n\
             \n\
             [Desktop Action run]\n\
             Name=Extract here\n\
             Exec=\"/opt/Pack Wing/packwing\" --extract 100%% %F\n"
        );
        assert_eq!(
            notes,
            [
                "Dolphin: no MIME type is known here for .rar, so \"Extract here\" does not show \
              for those files."
            ]
        );
    }

    #[test]
    fn items_for_nothing_known_are_left_out() {
        let d = dirs("dolphin-none", false);
        let menu = Menu::new("m", "M")
            .item(Item::new("a", "A", "/bin/a").extensions(["nope"]))
            .item(Item::new("b", "B", "/bin/b").count(Count::Many));
        let mut out = Vec::new();
        let mut notes = Vec::new();
        files(&menu, &d, &mut out, &mut notes);
        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("MimeType=application/octet-stream;\n"));
        assert!(out[0].text.contains("X-KDE-MinNumberOfUrls=2\n"));
        assert!(!out[0].text.contains("X-KDE-Submenu"));
        assert_eq!(notes.len(), 2);
    }
}
