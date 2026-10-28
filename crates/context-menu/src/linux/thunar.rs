//! Thunar (Xfce): custom actions, in the one file Thunar keeps them all in,
//! `Thunar/uca.xml` in the user's config folder. The user's own actions are
//! there too, so the menu's are merged in, as Thunar itself writes them, and
//! told apart by their unique ids, `context-menu-rs.<menu>.<item>`.
//!
//! Thunar reads the file when it starts and never again: it must restart
//! to show a change.

use std::fmt::Write as _;
use std::path::PathBuf;

use super::quote::shell;
use super::{Dirs, write};
use crate::{Count, Error, Files, Item, Menu, Report};

/// The user's file, which Thunar reads first.
fn path(dirs: &Dirs) -> PathBuf {
    dirs.config_home.join("Thunar/uca.xml")
}

/// The file Thunar would read without the user's: the system's, with its
/// sample actions, which the user's would otherwise hide.
fn system_file(dirs: &Dirs) -> Option<String> {
    dirs.config_dirs
        .iter()
        .find_map(|d| std::fs::read_to_string(d.join("Thunar/uca.xml")).ok())
}

/// What Thunar's file holds: the text up to its actions, each action with
/// the text before it, and the rest.
struct Actions {
    head: String,
    actions: Vec<(String, String)>,
    tail: String,
}

impl Actions {
    /// The actions of `text`, or none when it is not as Thunar writes it:
    /// one `<actions>` with plain `<action>` elements in it.
    fn parse(text: &str) -> Option<Actions> {
        let open = text.find("<actions>")? + "<actions>".len();
        let close = text.rfind("</actions>")?;
        let inner = text.get(open..close)?;
        let mut actions = Vec::new();
        let mut rest = inner;
        while let Some(start) = rest.find("<action>") {
            let end = rest[start..].find("</action>")? + start + "</action>".len();
            actions.push((rest[..start].to_string(), rest[start..end].to_string()));
            rest = &rest[end..];
        }
        // Any other way of writing an action, or one not closed, is not
        // understood: the file is left as it is.
        if inner.matches("<action").count() != actions.len() || rest.contains("</action>") {
            return None;
        }
        Some(Actions {
            head: text[..open].to_string(),
            actions,
            tail: format!("{rest}{}", &text[close..]),
        })
    }

    /// A file with no actions yet, as Thunar starts one.
    fn empty() -> Actions {
        Actions {
            head: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>".into(),
            actions: Vec::new(),
            tail: "\n</actions>\n".into(),
        }
    }

    fn text(&self) -> String {
        let mut s = self.head.clone();
        for (before, action) in &self.actions {
            s.push_str(before);
            s.push_str(action);
        }
        s.push_str(&self.tail);
        s
    }

    /// Whether `action` is one of the menu `id`'s.
    fn ours(action: &str, id: &str) -> bool {
        action.contains(&format!("<unique-id>context-menu-rs.{id}."))
    }
}

/// Puts `menu`'s actions in Thunar's file in place of an earlier install's,
/// or with none, takes them out. A file Thunar would not read as written is
/// left alone, with a note.
pub(super) fn update(
    id: &str,
    menu: Option<&Menu>,
    dirs: &Dirs,
    report: &mut Report,
) -> Result<(), Error> {
    let file = path(dirs);
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Error::io(&file, e)),
    };
    // Nothing to take out of a file there is not.
    if text.is_none() && menu.is_none() {
        return Ok(());
    }
    let base = match &text {
        Some(t) => Actions::parse(t),
        None => Some(
            system_file(dirs)
                .and_then(|t| Actions::parse(&t))
                .unwrap_or_else(Actions::empty),
        ),
    };
    let Some(mut actions) = base else {
        report.notes.push(format!(
            "Thunar: {} is not as Thunar writes it, so it is left as it is.",
            file.display()
        ));
        return Ok(());
    };
    actions.actions.retain(|(_, a)| !Actions::ours(a, id));
    if let Some(menu) = menu {
        for action in entries(menu) {
            actions.actions.push(("\n".into(), action));
        }
    }
    let new = actions.text();
    if text.as_deref() == Some(new.as_str()) {
        return Ok(());
    }
    write(&super::File {
        path: file.clone(),
        text: new,
        executable: false,
    })?;
    report.written.push(file);
    report
        .notes
        .push("Thunar: restart it (thunar -q) for the menu to show.".into());
    Ok(())
}

/// Whether Thunar's file holds `menu`'s actions as an install writes them,
/// and no others of the menu's.
pub(super) fn is_current(menu: &Menu, dirs: &Dirs) -> Result<bool, Error> {
    let file = path(dirs);
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::io(&file, e)),
    };
    // One install leaves alone is as current as it can be.
    let Some(actions) = Actions::parse(&text) else {
        return Ok(true);
    };
    let there: Vec<&str> = actions
        .actions
        .iter()
        .map(|(_, a)| a.as_str())
        .filter(|a| Actions::ours(a, &menu.id))
        .collect();
    Ok(there == entries(menu).iter().map(String::as_str).collect::<Vec<_>>())
}

/// The menu's actions. An item for some extensions and for folders is two:
/// Thunar matches its patterns against folders' names too.
fn entries(menu: &Menu) -> Vec<String> {
    let mut out = Vec::new();
    for item in &menu.items {
        let a = &item.applies_to;
        match &a.files {
            Files::Extensions(list) => {
                out.push(action(menu, item, "", &patterns(list), Kinds::Files));
                if a.folders {
                    out.push(action(menu, item, ".folders", "*", Kinds::Folders));
                }
            }
            Files::Any => {
                let kinds = if a.folders { Kinds::Both } else { Kinds::Files };
                out.push(action(menu, item, "", "*", kinds));
            }
            Files::None => out.push(action(menu, item, "", "*", Kinds::Folders)),
        }
    }
    out
}

/// Thunar's patterns for extensions: its matching minds case, so each in
/// lower and in upper case.
fn patterns(extensions: &[String]) -> String {
    let mut list: Vec<String> = Vec::new();
    for ext in extensions {
        for pattern in [format!("*.{ext}"), format!("*.{}", ext.to_uppercase())] {
            if !list.contains(&pattern) {
                list.push(pattern);
            }
        }
    }
    list.join(";")
}

/// What kinds of file an action is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kinds {
    Files,
    Folders,
    Both,
}

/// An action, as Thunar writes one (thunar-uca-model.c).
fn action(menu: &Menu, item: &Item, suffix: &str, patterns: &str, kinds: Kinds) -> String {
    let mut command = shell(&item.program.to_string_lossy());
    for arg in &item.args {
        command.push(' ');
        command.push_str(&shell(arg));
    }
    // %F, the paths chosen, also tells Thunar the action takes several.
    command.push_str(" %F");
    let range = match item.applies_to.count {
        Count::One => "1-1",
        // Thunar reads both ends as numbers.
        Count::Many => "2-1000000",
        Count::Any => "",
    };
    let submenu = if menu.submenu {
        menu.title.as_str()
    } else {
        ""
    };
    let mut s = String::from("<action>\n");
    let mut field = |name: &str, value: &str| {
        let _ = writeln!(s, "\t<{name}>{}</{name}>", markup(value));
    };
    field("icon", item.icon.as_deref().unwrap_or(""));
    field("name", &item.label);
    field("submenu", submenu);
    field(
        "unique-id",
        &format!("context-menu-rs.{}.{}{suffix}", menu.id, item.id),
    );
    field("command", &command);
    field("description", "");
    field("range", range);
    field("patterns", patterns);
    if kinds != Kinds::Files {
        s.push_str("\t<directories/>\n");
    }
    if kinds != Kinds::Folders {
        s.push_str(
            "\t<audio-files/>\n\t<image-files/>\n\t<other-files/>\n\t<text-files/>\n\t<video-files/>\n",
        );
    }
    s.push_str("</action>");
    s
}

/// Text escaped as `g_markup_printf_escaped` escapes it.
fn markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\'' => out.push_str("&apos;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {
                let _ = write!(out, "&#x{:x};", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::testing::dirs;

    fn menu() -> Menu {
        Menu::new("packwing", "Pack & Wing")
            .submenu(true)
            .item(
                Item::new("extract", "Extract <here>", "/opt/Pack Wing/packwing")
                    .args(["--to", "it's"])
                    .icon("package-x-generic")
                    .extensions(["zip", "tar.gz", "7z"])
                    .folders(true)
                    .count(Count::One),
            )
            .item(Item::new("any", "Anything", "/usr/bin/packwing").folders(true))
            .item(Item::new("two", "Compare", "/usr/bin/packwing").count(Count::Many))
    }

    #[test]
    fn items_are_written_as_thunar_writes_actions() {
        let all = entries(&menu());
        assert_eq!(all.len(), 4, "the first item is two");
        assert_eq!(
            all[0],
            "<action>\n\
             \t<icon>package-x-generic</icon>\n\
             \t<name>Extract &lt;here&gt;</name>\n\
             \t<submenu>Pack &amp; Wing</submenu>\n\
             \t<unique-id>context-menu-rs.packwing.extract</unique-id>\n\
             \t<command>&apos;/opt/Pack Wing/packwing&apos; &apos;--to&apos; &apos;it&apos;\\&apos;&apos;s&apos; %F</command>\n\
             \t<description></description>\n\
             \t<range>1-1</range>\n\
             \t<patterns>*.zip;*.ZIP;*.tar.gz;*.TAR.GZ;*.7z;*.7Z</patterns>\n\
             \t<audio-files/>\n\
             \t<image-files/>\n\
             \t<other-files/>\n\
             \t<text-files/>\n\
             \t<video-files/>\n\
             </action>"
        );
        assert!(all[1].contains("<unique-id>context-menu-rs.packwing.extract.folders</unique-id>"));
        assert!(all[1].contains("<patterns>*</patterns>\n\t<directories/>\n</action>"));
        assert!(all[2].contains("<patterns>*</patterns>\n\t<directories/>\n\t<audio-files/>"));
        assert!(all[3].contains("<range>2-1000000</range>"));
    }

    const USERS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <actions>\n\
        <action>\n\
        \t<name>Mine</name>\n\
        \t<unique-id>1700000000-1</unique-id>\n\
        \t<command>gimp %f</command>\n\
        </action>\n\
        </actions>\n";

    #[test]
    fn the_users_actions_stay() {
        let d = dirs("thunar");
        let file = path(&d);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, USERS).unwrap();

        let mut r = Report::default();
        update("packwing", Some(&menu()), &d, &mut r).unwrap();
        assert_eq!(r.written, std::slice::from_ref(&file));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with(USERS.trim_end_matches("</actions>\n")));
        assert_eq!(
            text.matches("<unique-id>context-menu-rs.packwing.").count(),
            4
        );
        assert!(is_current(&menu(), &d).unwrap());

        // Again, smaller: the menu's actions are replaced, the user's stay.
        let mut smaller = menu();
        smaller.items.truncate(1);
        update("packwing", Some(&smaller), &d, &mut Report::default()).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            text.matches("<unique-id>context-menu-rs.packwing.").count(),
            2
        );
        assert!(!is_current(&menu(), &d).unwrap());

        // Nothing changes: nothing is written.
        let mut r = Report::default();
        update("packwing", Some(&smaller), &d, &mut r).unwrap();
        assert!(r.written.is_empty());

        update("packwing", None, &d, &mut Report::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), USERS);
    }

    #[test]
    fn a_new_file_keeps_the_systems_actions() {
        let d = dirs("thunar-new");
        // Nothing to take out: no file is made.
        update("packwing", None, &d, &mut Report::default()).unwrap();
        assert!(!path(&d).exists());

        let system = d.config_dirs[0].join("Thunar/uca.xml");
        std::fs::create_dir_all(system.parent().unwrap()).unwrap();
        std::fs::write(
            &system,
            "<?xml version=\"1.0\"?>\n<!DOCTYPE actions [\n<!ELEMENT action (icon)*>\n]>\n\
             <!-- a comment -->\n<actions>\n\n  <action>\n    <name>Open Terminal Here</name>\n  </action>\n\n</actions>\n",
        )
        .unwrap();
        update("packwing", Some(&menu()), &d, &mut Report::default()).unwrap();
        let text = std::fs::read_to_string(path(&d)).unwrap();
        assert!(text.contains("<name>Open Terminal Here</name>"));
        assert!(text.contains("context-menu-rs.packwing.any"));
    }

    #[test]
    fn a_file_not_understood_is_left_alone() {
        let d = dirs("thunar-odd");
        let file = path(&d);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let odd = "<actions><action id=\"x\"><name>Odd</name></action></actions>";
        std::fs::write(&file, odd).unwrap();
        let mut r = Report::default();
        update("packwing", Some(&menu()), &d, &mut r).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), odd);
        assert!(r.notes[0].contains("left as it is"));
    }

    /// The file is XML a parser reads, with the values as written.
    #[test]
    fn the_file_is_xml() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let d = dirs("thunar-xml");
        update("packwing", Some(&menu()), &d, &mut Report::default()).unwrap();
        let out = std::process::Command::new("python3")
            .arg("-I")
            .arg("-c")
            .arg(
                "import sys, xml.etree.ElementTree as ET\n\
                 for a in ET.parse(sys.argv[1]).getroot():\n\
                 \x20   print(a.findtext('name'), '|', a.findtext('submenu'), '|', a.findtext('command'))",
            )
            .arg(path(&d))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let first = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert_eq!(
            first,
            "Extract <here> | Pack & Wing | '/opt/Pack Wing/packwing' '--to' 'it'\\''s' %F"
        );
    }
}
