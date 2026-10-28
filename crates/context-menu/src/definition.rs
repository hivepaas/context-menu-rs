//! A menu as text: what `install` writes for the native extensions, which
//! read it when the file manager asks what to show, so a changed menu needs
//! no new extension. The format is documented in `docs/definition.md`.
//!
//! A first line names the format and its version; then a line per field,
//! a key, a tab and a value. The menu's fields come first; an `item` line
//! starts an item, and the fields after it are that item's. In values a
//! backslash, a tab and a line break are written `\\`, `\t` and `\n`.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::{AppliesTo, Count, Error, Files, Item, Menu};

/// The first line; a newer version is refused.
const HEADER: &str = "context-menu-rs menu 1";

impl Menu {
    /// The menu in the definition format.
    pub fn to_definition(&self) -> String {
        let mut s = String::new();
        let mut field = |key: &str, value: &str| {
            let _ = writeln!(s, "{key}\t{}", escape(value));
        };
        field("id", &self.id);
        field("title", &self.title);
        if let Some(icon) = &self.icon {
            field("icon", icon);
        }
        field("submenu", if self.submenu { "true" } else { "false" });
        for item in &self.items {
            field("item", &item.id);
            field("label", &item.label);
            if let Some(icon) = &item.icon {
                field("icon", icon);
            }
            let a = &item.applies_to;
            match &a.files {
                Files::None => field("files", "none"),
                Files::Any => field("files", "any"),
                Files::Extensions(list) => {
                    field("files", "extensions");
                    for ext in list {
                        field("extension", ext);
                    }
                }
            }
            field("folders", if a.folders { "true" } else { "false" });
            field(
                "count",
                match a.count {
                    Count::One => "one",
                    Count::Many => "many",
                    Count::Any => "any",
                },
            );
            field("program", &item.program.to_string_lossy());
            for arg in &item.args {
                field("arg", arg);
            }
        }
        format!("{HEADER}\n{s}")
    }

    /// A menu from the definition format, checked as [`Menu::validate`]
    /// checks it. Keys it does not know are passed over, so a newer
    /// writer's additions do not stop an older reader.
    pub fn from_definition(text: &str) -> Result<Menu, Error> {
        let bad = |why: String| Error::Invalid(format!("menu definition: {why}"));
        let mut lines = text.lines().enumerate();
        match lines.next() {
            Some((_, HEADER)) => {}
            Some((_, first)) => return Err(bad(format!("not a version 1 definition: {first:?}"))),
            None => return Err(bad("empty".into())),
        }
        let mut menu = Menu::new("", "");
        for (n, line) in lines {
            if line.is_empty() {
                continue;
            }
            let (key, raw) = line
                .split_once('\t')
                .ok_or_else(|| bad(format!("line {}: no tab", n + 1)))?;
            let value = unescape(raw).ok_or_else(|| bad(format!("line {}: bad escape", n + 1)))?;
            let flag = |v: &str| match v {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(bad(format!("line {}: {key} is true or false", n + 1))),
            };
            if key == "item" {
                menu.items.push(Item::new(value, "", PathBuf::new()));
                continue;
            }
            let Some(item) = menu.items.last_mut() else {
                match key {
                    "id" => menu.id = value,
                    "title" => menu.title = value,
                    "icon" => menu.icon = Some(value),
                    "submenu" => menu.submenu = flag(&value)?,
                    _ => {}
                }
                continue;
            };
            let a: &mut AppliesTo = &mut item.applies_to;
            match key {
                "label" => item.label = value,
                "icon" => item.icon = Some(value),
                "files" => {
                    a.files = match value.as_str() {
                        "none" => Files::None,
                        "any" => Files::Any,
                        "extensions" => Files::Extensions(Vec::new()),
                        other => return Err(bad(format!("line {}: files {other:?}", n + 1))),
                    }
                }
                "extension" => match &mut a.files {
                    Files::Extensions(list) => list.push(value),
                    _ => {
                        return Err(bad(format!(
                            "line {}: an extension without files extensions",
                            n + 1
                        )));
                    }
                },
                "folders" => a.folders = flag(&value)?,
                "count" => {
                    a.count = match value.as_str() {
                        "one" => Count::One,
                        "many" => Count::Many,
                        "any" => Count::Any,
                        other => return Err(bad(format!("line {}: count {other:?}", n + 1))),
                    }
                }
                "program" => item.program = PathBuf::from(value),
                "arg" => item.args.push(value),
                _ => {}
            }
        }
        menu.validate()?;
        Ok(menu)
    }
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu {
        let program = if cfg!(windows) {
            r"C:\Program Files\App\app.exe"
        } else {
            "/opt/my app/app"
        };
        Menu::new("photo-kit", "Photo Kit")
            .submenu(true)
            .icon("camera-photo")
            .item(
                Item::new("resize", "Resize to 1080p", program)
                    .args(["--resize", "tab\there", r"back\slash"])
                    .icon("/usr/share/icons/x.png")
                    .extensions(["jpg", "tar.gz"])
                    .folders(true)
                    .count(Count::Many),
            )
            .item(
                Item::new("open", "Open", program)
                    .folders_only()
                    .count(Count::One),
            )
            .item(Item::new("any", "Anything", program))
    }

    #[test]
    fn a_menu_reads_back_as_it_was() {
        let m = menu();
        assert_eq!(Menu::from_definition(&m.to_definition()).unwrap(), m);
    }

    #[test]
    fn the_text_is_plain_lines() {
        let text = menu().to_definition();
        assert!(text.starts_with("context-menu-rs menu 1\nid\tphoto-kit\ntitle\tPhoto Kit\n"));
        assert!(text.contains("arg\ttab\\there\narg\tback\\\\slash\n"));
        assert!(text.contains("files\textensions\nextension\tjpg\nextension\ttar.gz\n"));
    }

    #[test]
    fn newer_keys_are_passed_over_and_bad_text_refused() {
        let text = menu()
            .to_definition()
            .replace("submenu\t", "colour\tred\nsubmenu\t");
        assert_eq!(Menu::from_definition(&text).unwrap(), menu());
        for bad in [
            "",
            "context-menu-rs menu 2\nid\tx\n",
            "context-menu-rs menu 1\nid x\n",
            "context-menu-rs menu 1\nid\tx\\q\n",
            "context-menu-rs menu 1\nid\tx\ntitle\tX\nsubmenu\tyes\n",
            // Valid lines, but no items: the menu is checked.
            "context-menu-rs menu 1\nid\tx\ntitle\tX\n",
        ] {
            assert!(Menu::from_definition(bad).is_err(), "{bad:?}");
        }
    }
}
