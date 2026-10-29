//! macOS: Finder's own menu, from the native extension an app's bundle
//! holds ([`FinderExtension`]); or without one, Finder's Quick Actions, a
//! workflow bundle per item in the user's Services folder,
//! `~/Library/Services`. Either way with no rights beyond the user's. See
//! `docs/macos.md`.
//!
//! The menu's definition goes in `~/Library/Application Support/context-menu-rs/<id>.menu`,
//! for the extension, which the system says serves the menu: the Quick
//! Actions are written only when none does.
//!
//! A bundle, `<menu>.<item>.workflow`, is what Automator saves for a Quick
//! Action: an `Info.plist` saying which file types (UTIs) Finder shows it
//! for, and a `document.wflow` whose one action, Run Shell Script, starts the
//! program with every path chosen, in one run. Finder shows the items in its
//! Quick Actions submenu, for any number of files chosen.
//!
//! The bundles are planned and written in plain Rust, the same on every
//! system. On macOS alone, the system says which types files with each
//! extension have, is told to turn the new Quick Actions on, which it leaves
//! off otherwise, and that the Services folder changed.

// The plan is used by `install`, on macOS, and by tests.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

mod finder;

pub use finder::{
    EXECUTABLE, EXTENSION_PATH, FinderExtension, GROUP_KEY, HELPER_PATH, MENU_KEY, PRINCIPAL_CLASS,
};

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Count, Error, Files, Item, Menu, Report};

/// The key in each bundle's `Info.plist` naming the menu it is of.
const MARK: &str = "ContextMenuRs";

/// Where the menu goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// The user's Services folder, `~/Library/Services`, which Finder's
    /// Quick Actions are read from.
    pub services: PathBuf,
    /// Where the system keeps which Quick Actions the user turned on: the
    /// preferences domain `pbs`, or the path of a property list without its
    /// `.plist`, as `defaults` takes it.
    pub status: String,
    /// `~/Library/Application Support`, where the menu's definition goes.
    pub data_home: PathBuf,
    /// The app bundle the running program is in, if it is in one: when it
    /// holds the native extension serving the menu, and the system does not
    /// know of it yet, `install` tells the system of it.
    pub app: Option<PathBuf>,
}

impl Root {
    /// The current user's, and the running program's app.
    pub fn current() -> Result<Root, Error> {
        let home = std::env::home_dir()
            .filter(|h| h.is_absolute())
            .ok_or(Error::NoHome)?;
        let app = std::env::current_exe().ok().and_then(|exe| {
            exe.ancestors()
                .find(|a| a.extension().is_some_and(|e| e == "app"))
                .map(Path::to_path_buf)
        });
        Ok(Root {
            services: home.join("Library/Services"),
            status: "pbs".into(),
            data_home: home.join("Library/Application Support"),
            app,
        })
    }
}

/// Where the definition of the menu `id` goes in `data_home`,
/// `~/Library/Application Support`: what the native extension reads.
pub fn definition_path(id: &str, data_home: &Path) -> PathBuf {
    data_home.join("context-menu-rs").join(format!("{id}.menu"))
}

/// Where the copies of the menu `id`'s icon files go in `data_home`, beside
/// its definition: the extension, in the sandbox, can read there, and not
/// in the app's bundle.
pub fn icons_path(id: &str, data_home: &Path) -> PathBuf {
    data_home.join("context-menu-rs").join(id)
}

/// The icon files `menu` names, by absolute path, and where each is copied:
/// `menu.<extension>` for the menu's, `item.<item>.<extension>` for an
/// item's. Ids hold no `.`, so no two are named alike.
fn icon_copies(menu: &Menu, root: &Root) -> Vec<(PathBuf, PathBuf)> {
    let dir = icons_path(&menu.id, &root.data_home);
    let named = std::iter::once(("menu".to_string(), menu.icon.as_ref())).chain(
        menu.items
            .iter()
            .map(|i| (format!("item.{}", i.id), i.icon.as_ref())),
    );
    named
        .filter_map(|(name, icon)| {
            let source = PathBuf::from(icon?.as_str());
            if !source.is_absolute() {
                return None;
            }
            let copy = match source.extension().and_then(|e| e.to_str()) {
                Some(extension) => dir.join(format!("{name}.{}", extension.to_lowercase())),
                None => dir.join(name),
            };
            Some((source, copy))
        })
        .collect()
}

/// `menu` as its definition has it on macOS: its icon files' copies in
/// place of them.
fn as_defined(menu: &Menu, root: &Root) -> Menu {
    let copies = icon_copies(menu, root);
    let copy = |icon: &Option<String>| {
        icon.as_ref().map(|icon| {
            copies
                .iter()
                .find(|(source, _)| source.as_os_str() == icon.as_str())
                .map_or_else(|| icon.clone(), |(_, copy)| copy.display().to_string())
        })
    };
    let mut defined = menu.clone();
    defined.icon = copy(&menu.icon);
    for item in &mut defined.items {
        item.icon = copy(&item.icon);
    }
    defined
}

/// Copies the icon files of `menu` beside its definition, those that changed
/// since an earlier install, and removes the copies it no longer names. A
/// file that cannot be read gets no copy, and so shows no icon, with a note.
fn copy_icons(menu: &Menu, root: &Root, report: &mut Report) -> Result<(), Error> {
    let copies = icon_copies(menu, root);
    let dir = icons_path(&menu.id, &root.data_home);
    for (source, copy) in &copies {
        let Ok(bytes) = fs::read(source) else {
            report.notes.push(format!(
                "Finder: the icon {} cannot be read, so it does not show.",
                source.display()
            ));
            remove(copy).map_err(|e| Error::io(copy, e))?;
            continue;
        };
        if fs::read(copy).is_ok_and(|there| there == bytes) {
            continue;
        }
        let tmp = dir.join(format!(
            ".{}.tmp",
            copy.file_name().unwrap_or_default().display()
        ));
        let result = fs::create_dir_all(&dir)
            .and_then(|()| fs::write(&tmp, &bytes))
            .and_then(|()| fs::rename(&tmp, copy));
        result.map_err(|e| {
            let _ = fs::remove_file(&tmp);
            Error::io(copy, e)
        })?;
        report.written.push(copy.clone());
    }
    for path in list(&dir)? {
        if !copies.iter().any(|(_, copy)| *copy == path) {
            remove(&path).map_err(|e| Error::io(&path, e))?;
            report.removed.push(path);
        }
    }
    if copies.is_empty() {
        let _ = fs::remove_dir(&dir);
    }
    Ok(())
}

/// Whether the copies of `menu`'s icon files are there as they are now, and
/// no others.
fn icons_current(menu: &Menu, root: &Root) -> Result<bool, Error> {
    let copies = icon_copies(menu, root);
    let mut there = list(&icons_path(&menu.id, &root.data_home))?;
    there.sort();
    let mut wanted: Vec<PathBuf> = copies
        .iter()
        .filter(|(source, _)| source.is_file())
        .map(|(_, copy)| copy.clone())
        .collect();
    wanted.sort();
    Ok(there == wanted
        && copies
            .iter()
            .all(|(source, copy)| fs::read(source).ok() == fs::read(copy).ok()))
}

/// The entries of `dir`, or none when it is not there.
fn list(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    match fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|e| e.map(|e| e.path()).map_err(|err| Error::io(dir, err)))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::io(dir, e)),
    }
}

/// A Finder extension serving a menu, as the system knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Served {
    /// Its bundle identifier.
    pub id: String,
    /// Where it is: `<app>/Contents/PlugIns/….appex`.
    pub path: PathBuf,
    /// Whether the user turned it on or off.
    pub election: Election,
}

/// What the user chose for an extension, in System Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Election {
    /// Turned on.
    On,
    /// Turned off.
    Off,
    /// Neither yet: it is off.
    None,
}

/// The extensions in `pluginkit -m -v`'s answer: a line each, starting
/// with the user's election (`+` on, `!` on for a debugger, `-` off, `?` or
/// nothing not yet), then the bundle identifier and version, and after tabs
/// its id, date and path. One another supersedes (`=`) is left out.
pub(crate) fn parse_plugins(text: &str) -> Vec<Served> {
    text.lines()
        .filter_map(|line| {
            let election = match line.chars().next()? {
                '+' | '!' => Election::On,
                '-' => Election::Off,
                ' ' | '?' => Election::None,
                _ => return None,
            };
            let mut fields = line[1..].trim_start().split('\t');
            let id = fields.next()?.rsplit_once('(')?.0.to_string();
            // A macOS path, whatever system reads it.
            let path = fields.nth(2).filter(|p| p.starts_with('/'))?;
            Some(Served {
                id,
                path: PathBuf::from(path),
                election,
            })
        })
        .collect()
}

/// What the report says of the extension serving the menu.
fn served_note(served: &Served) -> String {
    match served.election {
        Election::Off => format!(
            "Finder: the user turned off the extension serving the menu, {}, in System \
             Settings, so the menu does not show; there are no Quick Actions in its place.",
            served.id
        ),
        Election::On | Election::None => format!(
            "Finder: the menu is in Finder's own menu, from the extension {}.",
            served.id
        ),
    }
}

/// Writes the definition of `menu` under `root` whole, with its icon files'
/// copies: to a hidden file beside it first, then renamed over it, so the
/// extension never reads half of it.
fn write_definition(menu: &Menu, root: &Root) -> Result<PathBuf, Error> {
    let path = definition_path(&menu.id, &root.data_home);
    let dir = path.parent().unwrap_or(&root.data_home);
    let tmp = dir.join(format!(".{}.menu.tmp", menu.id));
    let result = fs::create_dir_all(dir)
        .and_then(|()| fs::write(&tmp, as_defined(menu, root).to_definition()))
        .and_then(|()| fs::rename(&tmp, &path));
    result.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        Error::io(&path, e)
    })?;
    Ok(path)
}

/// The file types of files with each extension, as the system names them:
/// by the extension, the last of an item's (`gz` for `tar.gz`), and for
/// each, the types a file so named may have.
pub(crate) type Types = BTreeMap<String, Vec<String>>;

/// A Quick Action: the bundle's name, and its two files; and the image file
/// it shows as its icon, copied into it as `Contents/Resources/icon.<ext>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bundle {
    pub name: String,
    pub label: String,
    pub info: String,
    pub document: String,
    pub icon: Option<PathBuf>,
}

/// What a Quick Action has for an icon: the name `Info.plist` gives it, and
/// the image file copied into the bundle under that name. Finder shows an
/// image in the bundle, and AppKit's named images (`NSFolder`), but not the
/// system's symbols: for those it shows a blank document, worse than the
/// default, so they are left out, with a note.
fn quick_icon(item: &Item, notes: &mut Vec<String>) -> (String, Option<PathBuf>) {
    const DEFAULT: &str = "NSActionTemplate";
    let Some(icon) = &item.icon else {
        return (DEFAULT.into(), None);
    };
    let path = Path::new(icon);
    if path.is_absolute() {
        if path.is_file() {
            return ("icon".into(), Some(path.to_path_buf()));
        }
        notes.push(format!(
            "Finder: the icon {icon} cannot be read, so {:?} has the default one.",
            item.label
        ));
    } else if icon.starts_with("NS") && fits(icon) {
        return (icon.clone(), None);
    } else {
        notes.push(format!(
            "Finder: Quick Actions show no symbols, so {:?} has the default icon, not \
             {icon:?}.",
            item.label
        ));
    }
    (DEFAULT.into(), None)
}

/// The name the image file copied into a bundle has there: `icon` and its
/// extension, which says its kind.
fn icon_file(source: &Path) -> String {
    match source.extension().and_then(|e| e.to_str()) {
        Some(extension) => format!("icon.{}", extension.to_lowercase()),
        None => "icon".into(),
    }
}

/// The last extension of `ext`, the one macOS knows a file by: `gz` for
/// `tar.gz`.
fn last(ext: &str) -> &str {
    ext.rsplit('.').next().unwrap_or(ext)
}

/// The extensions whose types an install of `menu` needs: the last of each
/// item's, each once.
pub(crate) fn extensions(menu: &Menu) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for item in &menu.items {
        if let Files::Extensions(list) = &item.applies_to.files {
            for ext in list {
                if !out.contains(&last(ext)) {
                    out.push(last(ext));
                }
            }
        }
    }
    out
}

/// The bundles an install of `menu` writes, and its notes.
pub(crate) fn plan(menu: &Menu, types: &Types) -> (Vec<Bundle>, Vec<String>) {
    let mut notes = vec![if menu.submenu {
        format!(
            "Finder: the items are in its Quick Actions submenu, not in one of their own \
             named {:?}.",
            menu.title
        )
    } else {
        "Finder: the items are in its Quick Actions submenu.".into()
    }];
    let mut bundles = Vec::new();
    for item in &menu.items {
        let Some(file_types) = file_types(item, types, &mut notes) else {
            continue;
        };
        let program = item.program.to_string_lossy();
        if !fits(&item.label) || !fits(&program) || !item.args.iter().all(|a| fits(a)) {
            notes.push(format!(
                "Finder: {:?} is left out: its label, program or an argument holds a character \
                 a property list cannot.",
                item.label
            ));
            continue;
        }
        match item.applies_to.count {
            Count::One => notes.push(format!(
                "Finder: {:?} shows for several files chosen too, and runs with all of them.",
                item.label
            )),
            Count::Many => notes.push(format!(
                "Finder: {:?} shows for one file chosen too.",
                item.label
            )),
            Count::Any => {}
        }
        let (icon_name, icon) = quick_icon(item, &mut notes);
        bundles.push(Bundle {
            name: format!("{}.{}.workflow", menu.id, item.id),
            label: item.label.clone(),
            info: info(menu, item, &file_types, &icon_name),
            document: document(item),
            icon,
        });
    }
    (bundles, notes)
}

/// The types `item` shows for, or none when it would show for nothing (a
/// note says why).
fn file_types(item: &Item, types: &Types, notes: &mut Vec<String>) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |t: &str| {
        if !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    };
    match &item.applies_to.files {
        Files::None => {}
        // Files, and the folders Finder shows as files: apps, and documents
        // that are packages.
        Files::Any => {
            add("public.data");
            add("com.apple.package");
        }
        Files::Extensions(list) => {
            for ext in list {
                let last = last(ext);
                if last != ext && !list.iter().any(|e| e == last) {
                    notes.push(format!(
                        "Finder: files are known by their last extension, so {:?} shows for \
                         every .{last} file, not only .{ext}.",
                        item.label
                    ));
                }
                match types.get(last) {
                    Some(found) if !found.is_empty() => found.iter().for_each(|t| add(t)),
                    _ => notes.push(format!(
                        "Finder: no file type is known here for .{last}, so {:?} does not \
                         show for those files.",
                        item.label
                    )),
                }
            }
        }
    }
    if item.applies_to.folders {
        add("public.folder");
    }
    if out.is_empty() {
        notes.push(format!("Finder: {:?} is left out.", item.label));
        return None;
    }
    Some(out)
}

/// Whether `s` can be in a property list: XML 1.0 has no way to write most
/// control characters.
fn fits(s: &str) -> bool {
    s.chars().all(|c| {
        !matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}')
    })
}

/// Text in a property list's XML.
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A word for a POSIX shell: in single quotes, a quote as `'\''`.
fn shell(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// What Run Shell Script runs, with the paths chosen as its arguments: the
/// program, its arguments and the paths, started and not waited for, as
/// other file managers start it, so the Quick Action ends at once. With
/// job control on (`set -m`), it runs in a process group of its own, so it
/// outlives the one running the Quick Action; and with nothing to read
/// from it, Run Shell Script waits for none of its output.
fn command(item: &Item) -> String {
    let mut s = format!("set -m; {}", shell(&item.program.to_string_lossy()));
    for arg in &item.args {
        s.push(' ');
        s.push_str(&shell(arg));
    }
    s.push_str(" \"$@\" </dev/null >/dev/null 2>&1 &");
    s
}

/// The bundle's `Info.plist`: the service, as Automator declares a Quick
/// Action, for Finder alone and the types given; and the menu's mark.
fn info(menu: &Menu, item: &Item, file_types: &[String], icon: &str) -> String {
    let mut types = String::new();
    for t in file_types {
        let _ = writeln!(types, "\t\t\t\t<string>{}</string>", xml(t));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>{MARK}</key>
	<string>{id}</string>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSBackgroundColorName</key>
			<string>background</string>
			<key>NSIconName</key>
			<string>{icon}</string>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{label}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
{types}			</array>
		</dict>
	</array>
</dict>
</plist>
"#,
        id = menu.id,
        label = xml(&item.label),
        icon = xml(icon),
    )
}

/// The bundle's `document.wflow`: a workflow for Finder's files and folders,
/// as Automator saves one, of one action, Run Shell Script, given the paths
/// as arguments. Its ids are the same in every bundle: they only need to be
/// unique within one.
fn document(item: &Item) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>540</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{command}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/sh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>8E0F2C3A-6D0B-4C59-9A41-3B6F3F0C2A01</string>
				<key>Keywords</key>
				<array>
					<string>Shell</string>
					<string>Script</string>
					<string>Command</string>
					<string>Run</string>
					<string>Unix</string>
				</array>
				<key>OutputUUID</key>
				<string>8E0F2C3A-6D0B-4C59-9A41-3B6F3F0C2A02</string>
				<key>UUID</key>
				<string>8E0F2C3A-6D0B-4C59-9A41-3B6F3F0C2A03</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
				<key>arguments</key>
				<dict>
					<key>0</key>
					<dict>
						<key>default value</key>
						<integer>0</integer>
						<key>name</key>
						<string>inputMethod</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>0</string>
					</dict>
					<key>1</key>
					<dict>
						<key>default value</key>
						<string></string>
						<key>name</key>
						<string>source</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>1</string>
					</dict>
					<key>2</key>
					<dict>
						<key>default value</key>
						<false/>
						<key>name</key>
						<string>CheckedForUserDefaultShell</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>2</string>
					</dict>
					<key>3</key>
					<dict>
						<key>default value</key>
						<string></string>
						<key>name</key>
						<string>COMMAND_STRING</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>3</string>
					</dict>
					<key>4</key>
					<dict>
						<key>default value</key>
						<string>/bin/sh</string>
						<key>name</key>
						<string>shell</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>4</string>
					</dict>
				</dict>
				<key>isViewVisible</key>
				<true/>
				<key>location</key>
				<string>529.000000:305.000000</string>
				<key>nibPath</key>
				<string>/System/Library/Automator/Run Shell Script.action/Contents/Resources/Base.lproj/main.nib</string>
			</dict>
			<key>isViewVisible</key>
			<true/>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>applicationBundleID</key>
		<string>com.apple.finder</string>
		<key>applicationBundleIDsByPath</key>
		<dict>
			<key>/System/Library/CoreServices/Finder.app</key>
			<string>com.apple.finder</string>
		</dict>
		<key>applicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>applicationPaths</key>
		<array>
			<string>/System/Library/CoreServices/Finder.app</string>
		</array>
		<key>inputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>outputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>presentationMode</key>
		<integer>15</integer>
		<key>processesInput</key>
		<integer>0</integer>
		<key>serviceApplicationBundleID</key>
		<string>com.apple.finder</string>
		<key>serviceApplicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<integer>0</integer>
		<key>systemImageName</key>
		<string>NSActionTemplate</string>
		<key>useAutomaticInputType</key>
		<integer>0</integer>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#,
        command = xml(&command(item)),
    )
}

/// The bundles of the menu `id` there are: those in the Services folder
/// named `<id>.*.workflow`. Ids hold no `.`, so another menu's never match.
fn existing(id: &str, root: &Root) -> Result<Vec<PathBuf>, Error> {
    let entries = match fs::read_dir(&root.services) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(&root.services, e)),
    };
    let prefix = format!("{id}.");
    let mut found = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| Error::io(&root.services, e))?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with(&prefix) && name.ends_with(".workflow") {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

/// Writes `bundle` in `dir` whole: in a hidden folder beside it first,
/// which takes the place of an earlier one, so the system never reads half
/// of one.
fn write(bundle: &Bundle, dir: &Path) -> Result<PathBuf, Error> {
    let path = dir.join(&bundle.name);
    let tmp = dir.join(format!(".{}.tmp", bundle.name));
    let result = (|| {
        remove(&tmp)?;
        let contents = tmp.join("Contents");
        fs::create_dir_all(&contents)?;
        fs::write(contents.join("Info.plist"), &bundle.info)?;
        fs::write(contents.join("document.wflow"), &bundle.document)?;
        if let Some(source) = &bundle.icon {
            let resources = contents.join("Resources");
            fs::create_dir_all(&resources)?;
            fs::copy(source, resources.join(icon_file(source)))?;
        }
        remove(&path)?;
        fs::rename(&tmp, &path)
    })();
    result.map_err(|e| {
        let _ = fs::remove_dir_all(&tmp);
        Error::io(&path, e)
    })?;
    Ok(path)
}

/// Removes `path`, a folder and all in it or a file, when it is there.
fn remove(path: &Path) -> std::io::Result<()> {
    let removed = match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) => Err(e),
    };
    match removed {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The Quick Actions an install of `menu` writes, with the file types
/// `types`, and its notes: none when an extension serves the menu.
fn wanted(menu: &Menu, types: &Types, served: Option<&Served>) -> (Vec<Bundle>, Vec<String>) {
    match served {
        Some(served) => (Vec::new(), vec![served_note(served)]),
        None => plan(menu, types),
    }
}

/// Installs `menu` under `root` in place of an earlier install: its
/// definition, and its Quick Actions, with the file types `types`, unless
/// the extension `served` serves it. The bundles the earlier install wrote
/// and this one does not are removed.
pub(crate) fn install_with(
    menu: &Menu,
    root: &Root,
    types: &Types,
    served: Option<&Served>,
) -> Result<Report, Error> {
    menu.validate()?;
    let (bundles, notes) = wanted(menu, types, served);
    let mut report = Report {
        notes,
        ..Report::default()
    };
    for path in existing(&menu.id, root)? {
        if !bundles.iter().any(|b| root.services.join(&b.name) == path) {
            remove(&path).map_err(|e| Error::io(&path, e))?;
            report.removed.push(path);
        }
    }
    for bundle in &bundles {
        report.written.push(write(bundle, &root.services)?);
    }
    copy_icons(menu, root, &mut report)?;
    report.written.push(write_definition(menu, root)?);
    Ok(report)
}

/// Takes the bundles, the definition and the icons' copies of the menu
/// `id` away from under `root`.
pub(crate) fn uninstall_from(id: &str, root: &Root) -> Result<Report, Error> {
    let mut report = Report::default();
    let mut paths = existing(id, root)?;
    for path in [
        definition_path(id, &root.data_home),
        icons_path(id, &root.data_home),
    ] {
        if fs::symlink_metadata(&path).is_ok() {
            paths.push(path);
        }
    }
    for path in paths {
        remove(&path).map_err(|e| Error::io(&path, e))?;
        report.removed.push(path);
    }
    Ok(report)
}

/// Whether what is under `root` is what [`install_with`] would write for
/// `menu`, `types` and `served`, and nothing else of the menu's; and an
/// extension serving it has been turned on or off.
pub(crate) fn is_current_with(
    menu: &Menu,
    root: &Root,
    types: &Types,
    served: Option<&Served>,
) -> Result<bool, Error> {
    menu.validate()?;
    if served.is_some_and(|s| s.election == Election::None) {
        return Ok(false);
    }
    let definition = fs::read_to_string(definition_path(&menu.id, &root.data_home));
    if definition.ok().as_deref() != Some(as_defined(menu, root).to_definition().as_str())
        || !icons_current(menu, root)?
    {
        return Ok(false);
    }
    let (bundles, _) = wanted(menu, types, served);
    let there = existing(&menu.id, root)?;
    let mut planned: Vec<PathBuf> = bundles
        .iter()
        .map(|b| root.services.join(&b.name))
        .collect();
    planned.sort();
    if there != planned {
        return Ok(false);
    }
    Ok(bundles.iter().all(|b| {
        let contents = root.services.join(&b.name).join("Contents");
        let read = |name: &str| fs::read_to_string(contents.join(name)).ok();
        let resources: Vec<PathBuf> = list(&contents.join("Resources")).unwrap_or_default();
        let icon = match &b.icon {
            Some(source) => {
                let copy = contents.join("Resources").join(icon_file(source));
                resources == [copy.clone()] && fs::read(source).ok() == fs::read(&copy).ok()
            }
            None => resources.is_empty(),
        };
        read("Info.plist").as_deref() == Some(b.info.as_str())
            && read("document.wflow").as_deref() == Some(b.document.as_str())
            && icon
    }))
}

#[cfg(target_os = "macos")]
mod system;

/// Installs `menu` under `root`, in place of an earlier install: its
/// definition, and, when no extension serves it, its Quick Actions, with the
/// file types the system knows for its extensions now. An extension serving
/// it in `root.app` is told of to the system, and turned on, as are the
/// Quick Actions, when the user has not turned it on or off.
#[cfg(target_os = "macos")]
pub fn install(menu: &Menu, root: &Root) -> Result<Report, Error> {
    menu.validate()?;
    // The app's own extension is told of again each time: once the app is
    // replaced, by an update, the system's record of it may be gone, or
    // about to go.
    if let Some(app) = root.app.as_deref() {
        system::register(&menu.id, app);
    }
    let served = system::served(&menu.id);
    let types = match served {
        Some(_) => Types::new(),
        None => system::types(&extensions(menu))?,
    };
    let mut report = install_with(menu, root, &types, served.as_ref())?;
    match &served {
        Some(s) if s.election == Election::None => {
            if let Err(e) = system::turn_on_extension(&s.id) {
                report.notes.push(format!(
                    "Finder: the extension could not be turned on ({e}): System Settings, \
                     under Login Items & Extensions, turns it on."
                ));
            }
        }
        Some(_) => {}
        None => {
            let (bundles, _) = plan(menu, &types);
            let labels: Vec<&str> = bundles.iter().map(|b| b.label.as_str()).collect();
            if let Err(e) = system::turn_on(&labels, &root.status) {
                report.notes.push(format!(
                    "Finder: the items could not be turned on ({e}): Customize…, in its \
                     Quick Actions submenu, turns them on."
                ));
            }
        }
    }
    if served.is_none() || !report.removed.is_empty() {
        system::refresh();
    }
    Ok(report)
}

/// Takes the menu `id` away from under `root`: its Quick Actions and its
/// definition, so an extension serving it shows nothing.
#[cfg(target_os = "macos")]
pub fn uninstall(id: &str, root: &Root) -> Result<Report, Error> {
    let report = uninstall_from(id, root)?;
    if report.removed.iter().any(|p| p.starts_with(&root.services)) {
        system::refresh();
    }
    Ok(report)
}

/// Whether what is under `root` holds `menu` as [`install`] writes it now,
/// and nothing else of the menu's: false too when an extension serving it
/// came or went, or is in `root.app` and unknown to the system, or has not
/// been turned on or off; or, with no extension, when the file types the
/// system knows for its extensions changed, as when an app declaring one is
/// installed.
#[cfg(target_os = "macos")]
pub fn is_current(menu: &Menu, root: &Root) -> Result<bool, Error> {
    menu.validate()?;
    let served = system::served(&menu.id);
    if served.is_none()
        && root
            .app
            .as_deref()
            .is_some_and(|app| system::bundled(&menu.id, app))
    {
        return Ok(false);
    }
    let types = match served {
        Some(_) => Types::new(),
        None => system::types(&extensions(menu))?,
    };
    is_current_with(menu, root, &types, served.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// An absolute path on this system: Windows's have a drive.
    const APP: &str = if cfg!(windows) {
        r"C:\Pack Wing\packwing.exe"
    } else {
        "/Applications/Pack Wing.app/Contents/MacOS/packwing"
    };

    /// A Services folder in a fresh folder under the system's temporary
    /// one, for one test.
    fn root(name: &str) -> Root {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "context-menu-test-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Root {
            services: dir.join("Services"),
            status: dir.join("status").display().to_string(),
            data_home: dir.join("Application Support"),
            app: None,
        }
    }

    /// An extension serving the menu, with the user's election.
    fn served(election: Election) -> Served {
        Served {
            id: "com.example.packwing.context-menu".into(),
            path: "/Applications/Pack Wing.app/Contents/PlugIns/ContextMenu.appex".into(),
            election,
        }
    }

    fn types() -> Types {
        [
            ("zip", "public.zip-archive"),
            ("7z", "org.7-zip.7-zip-archive"),
            ("gz", "org.gnu.gnu-zip-archive"),
        ]
        .into_iter()
        .map(|(ext, t)| (ext.to_string(), vec![t.to_string()]))
        .collect()
    }

    fn menu(submenu: bool) -> Menu {
        Menu::new("packwing", "Pack Wing")
            .submenu(submenu)
            .item(
                Item::new("extract", "Extract here", APP)
                    .args(["--extract", "it's"])
                    .extensions(["zip", "7z", "tar.gz"]),
            )
            .item(Item::new("compress", "Compress", APP).folders(true))
            .item(Item::new("compare", "Compare <the two> & more", APP).count(Count::Many))
    }

    #[test]
    fn an_item_is_a_quick_action() {
        let (bundles, _) = plan(&menu(true), &types());
        let names: Vec<&str> = bundles.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "packwing.extract.workflow",
                "packwing.compress.workflow",
                "packwing.compare.workflow",
            ]
        );
        let info = &bundles[0].info;
        assert!(info.contains("\t<key>ContextMenuRs</key>\n\t<string>packwing</string>\n"));
        assert!(
            info.contains("\t\t\t\t<key>default</key>\n\t\t\t\t<string>Extract here</string>\n")
        );
        assert!(info.contains(
            "\t\t\t<key>NSSendFileTypes</key>\n\
             \t\t\t<array>\n\
             \t\t\t\t<string>public.zip-archive</string>\n\
             \t\t\t\t<string>org.7-zip.7-zip-archive</string>\n\
             \t\t\t\t<string>org.gnu.gnu-zip-archive</string>\n\
             \t\t\t</array>\n"
        ));
        assert!(bundles[0].document.contains(&format!(
            "<string>set -m; '{APP}' '--extract' 'it'\\''s' \"$@\" \
             &lt;/dev/null &gt;/dev/null 2&gt;&amp;1 &amp;</string>"
        )));
        assert!(bundles[1].info.contains(
            "\t\t\t\t<string>public.data</string>\n\
             \t\t\t\t<string>com.apple.package</string>\n\
             \t\t\t\t<string>public.folder</string>\n"
        ));
        assert!(
            bundles[2]
                .info
                .contains("<string>Compare &lt;the two&gt; &amp; more</string>")
        );
    }

    #[test]
    fn notes_say_what_finder_cannot_do() {
        let (_, notes) = plan(&menu(true), &types());
        assert_eq!(
            notes,
            [
                "Finder: the items are in its Quick Actions submenu, not in one of their own \
                 named \"Pack Wing\".",
                "Finder: files are known by their last extension, so \"Extract here\" shows for \
                 every .gz file, not only .tar.gz.",
                "Finder: \"Compare <the two> & more\" shows for one file chosen too.",
            ]
        );
        let (_, notes) = plan(&menu(false), &types());
        assert_eq!(
            notes[0],
            "Finder: the items are in its Quick Actions submenu."
        );

        // No type known, or a program no property list can hold: left out.
        let menu = Menu::new("m", "M")
            .item(Item::new("a", "A", "/bin/a").extensions(["nope"]))
            .item(Item::new("b", "B", "/bin/b").arg("\u{1}"))
            .item(Item::new("c", "C", "/bin/c").count(Count::One));
        let (bundles, notes) = plan(&menu, &types());
        assert_eq!(bundles.len(), 1);
        assert_eq!(
            notes[1..],
            [
                "Finder: no file type is known here for .nope, so \"A\" does not show for those \
                 files.",
                "Finder: \"A\" is left out.",
                "Finder: \"B\" is left out: its label, program or an argument holds a character \
                 a property list cannot.",
                "Finder: \"C\" shows for several files chosen too, and runs with all of them.",
            ]
        );
        assert_eq!(extensions(&menu), ["nope"]);
    }

    #[test]
    fn installing_again_replaces_and_uninstall_takes_all_away() {
        let r = root("install");
        let report = install_with(&menu(true), &r, &types(), None).unwrap();
        assert_eq!(report.written.len(), 4, "three bundles and the definition");
        assert!(report.removed.is_empty());
        let definition = definition_path("packwing", &r.data_home);
        assert_eq!(report.written[3], definition);
        assert_eq!(
            Menu::from_definition(&fs::read_to_string(&definition).unwrap()).unwrap(),
            menu(true)
        );
        let bundle = r.services.join("packwing.extract.workflow/Contents");
        assert!(bundle.join("Info.plist").is_file() && bundle.join("document.wflow").is_file());

        // A file where a bundle of the menu's goes is replaced.
        let stray = r.services.join("packwing.compress.workflow");
        fs::remove_dir_all(&stray).unwrap();
        fs::write(&stray, "x").unwrap();

        // Another menu's bundle, and the user's own, stay.
        let others = [
            r.services.join("packwingx.thing.workflow"),
            r.services.join("My action.workflow"),
        ];
        for o in &others {
            fs::create_dir_all(o.join("Contents")).unwrap();
        }

        // One item fewer: its bundle goes, the rest are written again.
        let mut smaller = menu(true);
        smaller.items.pop();
        let report = install_with(&smaller, &r, &types(), None).unwrap();
        assert_eq!(
            report.removed,
            [r.services.join("packwing.compare.workflow")]
        );
        assert_eq!(report.written.len(), 3);
        assert!(stray.join("Contents/Info.plist").is_file());
        let mut left: Vec<String> = fs::read_dir(&r.services)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "My action.workflow",
                "packwing.compress.workflow",
                "packwing.extract.workflow",
                "packwingx.thing.workflow",
            ]
        );

        let report = uninstall_from("packwing", &r).unwrap();
        assert_eq!(report.removed.len(), 3, "two bundles and the definition");
        assert!(!definition.exists());
        assert!(others.iter().all(|o| o.exists()));
        assert!(uninstall_from("packwing", &r).unwrap().removed.is_empty());
    }

    #[test]
    fn current_until_the_menu_or_the_types_change() {
        let r = root("current");
        assert!(
            !is_current_with(&menu(true), &r, &types(), None).unwrap(),
            "nothing installed"
        );
        install_with(&menu(true), &r, &types(), None).unwrap();
        assert!(is_current_with(&menu(true), &r, &types(), None).unwrap());
        let mut renamed = menu(true);
        renamed.items[0].label = "Extract".into();
        assert!(
            !is_current_with(&renamed, &r, &types(), None).unwrap(),
            "the menu changed"
        );

        // An app declares 7z's type: the filter must change.
        let mut declared = types();
        declared.insert("7z".into(), vec!["com.example.7z".into()]);
        assert!(!is_current_with(&menu(true), &r, &declared, None).unwrap());

        // A bundle changed by hand, or one gone.
        let info = r
            .services
            .join("packwing.compress.workflow/Contents/Info.plist");
        fs::write(&info, "changed").unwrap();
        assert!(!is_current_with(&menu(true), &r, &types(), None).unwrap());
        install_with(&menu(true), &r, &types(), None).unwrap();
        fs::remove_dir_all(r.services.join("packwing.compare.workflow")).unwrap();
        assert!(!is_current_with(&menu(true), &r, &types(), None).unwrap());
    }

    #[test]
    fn an_extension_serving_the_menu_takes_the_quick_actions_place() {
        let r = root("served");
        install_with(&menu(true), &r, &types(), None).unwrap();
        let on = served(Election::On);
        assert!(!is_current_with(&menu(true), &r, &types(), Some(&on)).unwrap());

        // The extension serves it: the Quick Actions go, the definition stays.
        let report = install_with(&menu(true), &r, &types(), Some(&on)).unwrap();
        assert_eq!(report.removed.len(), 3, "{:?}", report.removed);
        assert_eq!(report.written, [definition_path("packwing", &r.data_home)]);
        assert_eq!(
            report.notes,
            [
                "Finder: the menu is in Finder's own menu, from the extension \
              com.example.packwing.context-menu."
            ]
        );
        assert!(existing("packwing", &r).unwrap().is_empty());
        assert!(is_current_with(&menu(true), &r, &types(), Some(&on)).unwrap());

        // Not turned on or off yet: install turns it on.
        let none = served(Election::None);
        assert!(!is_current_with(&menu(true), &r, &types(), Some(&none)).unwrap());
        // Turned off: the user's choice, with no Quick Actions in its place.
        let off = served(Election::Off);
        assert!(is_current_with(&menu(true), &r, &types(), Some(&off)).unwrap());
        let report = install_with(&menu(true), &r, &types(), Some(&off)).unwrap();
        assert!(report.notes[0].contains("turned off"), "{:?}", report.notes);

        // A changed menu is a changed definition.
        let mut renamed = menu(true);
        renamed.items[0].label = "Extract".into();
        assert!(!is_current_with(&renamed, &r, &types(), Some(&on)).unwrap());

        // The extension gone: the Quick Actions again.
        assert!(!is_current_with(&menu(true), &r, &types(), None).unwrap());
        install_with(&menu(true), &r, &types(), None).unwrap();
        assert_eq!(existing("packwing", &r).unwrap().len(), 3);
    }

    #[test]
    fn icon_files_are_copied_where_the_extension_can_read_them() {
        let r = root("icons");
        let resources = r
            .data_home
            .parent()
            .unwrap()
            .join("Pack Wing.app/Resources");
        fs::create_dir_all(&resources).unwrap();
        let zip = resources.join("zip.PNG");
        let folder = resources.join("folder.icns");
        fs::write(&zip, "png 1").unwrap();
        fs::write(&folder, "icns").unwrap();
        let menu = |zip_icon: bool| {
            let mut extract = Item::new("extract", "Extract", APP).extensions(["zip"]);
            if zip_icon {
                extract = extract.icon(zip.display().to_string());
            }
            Menu::new("packwing", "Pack Wing")
                .submenu(true)
                .icon(folder.display().to_string())
                .item(extract)
                .item(Item::new("menu", "Other", APP).icon("archivebox"))
        };
        let on = served(Election::On);
        let icons = icons_path("packwing", &r.data_home);
        let menu_copy = icons.join("menu.icns");
        let zip_copy = icons.join("item.extract.png");

        let report = install_with(&menu(true), &r, &types(), Some(&on)).unwrap();
        assert!(report.written.contains(&menu_copy) && report.written.contains(&zip_copy));
        assert_eq!(fs::read_to_string(&zip_copy).unwrap(), "png 1");
        // The definition names the copies, and the symbol as it is.
        let defined = Menu::from_definition(
            &fs::read_to_string(definition_path("packwing", &r.data_home)).unwrap(),
        )
        .unwrap();
        assert_eq!(defined.icon, Some(menu_copy.display().to_string()));
        assert_eq!(defined.items[0].icon, Some(zip_copy.display().to_string()));
        assert_eq!(defined.items[1].icon.as_deref(), Some("archivebox"));
        assert!(is_current_with(&menu(true), &r, &types(), Some(&on)).unwrap());

        // The app's icon changed: copied again, and only it.
        fs::write(&zip, "png 2").unwrap();
        assert!(!is_current_with(&menu(true), &r, &types(), Some(&on)).unwrap());
        let report = install_with(&menu(true), &r, &types(), Some(&on)).unwrap();
        assert!(report.written.contains(&zip_copy) && !report.written.contains(&menu_copy));
        assert_eq!(fs::read_to_string(&zip_copy).unwrap(), "png 2");

        // An icon no longer named: its copy goes.
        let report = install_with(&menu(false), &r, &types(), Some(&on)).unwrap();
        assert_eq!(report.removed, std::slice::from_ref(&zip_copy));
        assert!(is_current_with(&menu(false), &r, &types(), Some(&on)).unwrap());

        // An icon that cannot be read: no copy, a note, and still current.
        fs::remove_file(&folder).unwrap();
        let report = install_with(&menu(false), &r, &types(), Some(&on)).unwrap();
        assert!(!menu_copy.exists());
        assert!(
            report.notes.iter().any(|n| n.contains("cannot be read")),
            "{:?}",
            report.notes
        );
        assert!(is_current_with(&menu(false), &r, &types(), Some(&on)).unwrap());

        // Uninstall takes the copies too.
        fs::write(&folder, "icns").unwrap();
        install_with(&menu(true), &r, &types(), Some(&on)).unwrap();
        let report = uninstall_from("packwing", &r).unwrap();
        assert!(report.removed.contains(&icons));
        assert!(!icons.exists());
    }

    #[test]
    fn quick_actions_show_image_files_and_appkits_images() {
        let r = root("quick-icons");
        let zip = r.data_home.parent().unwrap().join("zip.PNG");
        fs::write(&zip, "png 1").unwrap();
        let menu = Menu::new("packwing", "Pack Wing")
            .item(Item::new("file", "File", APP).icon(zip.display().to_string()))
            .item(Item::new("named", "Named", APP).icon("NSFolder"))
            .item(Item::new("symbol", "Symbol", APP).icon("archivebox"))
            .item(
                Item::new("gone", "Gone", APP)
                    .icon(zip.with_file_name("gone.png").display().to_string()),
            );
        let (bundles, notes) = plan(&menu, &types());
        let icon = |b: &Bundle, name: &str| {
            b.info.contains(&format!(
                "\t\t\t<key>NSIconName</key>\n\t\t\t<string>{name}</string>\n"
            ))
        };
        assert!(icon(&bundles[0], "icon") && bundles[0].icon.as_deref() == Some(zip.as_path()));
        assert!(icon(&bundles[1], "NSFolder") && bundles[1].icon.is_none());
        assert!(icon(&bundles[2], "NSActionTemplate") && bundles[2].icon.is_none());
        assert!(icon(&bundles[3], "NSActionTemplate"));
        assert!(
            notes
                .iter()
                .any(|n| n.contains("Quick Actions show no symbols, so \"Symbol\""))
        );
        assert!(
            notes
                .iter()
                .any(|n| n.contains("cannot be read, so \"Gone\""))
        );

        install_with(&menu, &r, &types(), None).unwrap();
        let resources = r.services.join("packwing.file.workflow/Contents/Resources");
        assert_eq!(
            fs::read_to_string(resources.join("icon.png")).unwrap(),
            "png 1"
        );
        assert!(
            !r.services
                .join("packwing.named.workflow/Contents/Resources")
                .exists()
        );
        assert!(is_current_with(&menu, &r, &types(), None).unwrap());

        // The app's icon changed: not current, and copied again.
        fs::write(&zip, "png 2").unwrap();
        assert!(!is_current_with(&menu, &r, &types(), None).unwrap());
        install_with(&menu, &r, &types(), None).unwrap();
        assert_eq!(
            fs::read_to_string(resources.join("icon.png")).unwrap(),
            "png 2"
        );
        assert!(is_current_with(&menu, &r, &types(), None).unwrap());
    }

    #[test]
    fn pluginkit_says_which_extension_and_its_election() {
        let text = "+    com.example.packwing.context-menu(0.1)\t9EC808FA-B88A\t\
                    2026-10-29 06:37:32 +0000\t/Applications/Pack Wing.app/Contents/PlugIns/\
                    ContextMenu.appex\n\
                    -    com.example.other.context-menu(1.0)\tAB\t2026-10-29\t/Applications/\
                    Other.app/Contents/PlugIns/ContextMenu.appex\n\
                    \x20    com.example.third.context-menu(2)\tCD\t2026-10-29\t/tmp/T.app/x.appex\n\
                    =    com.example.old.context-menu(1)\tEF\t2026-10-28\t/tmp/Old.app/x.appex\n\
                    ?    com.example.fourth.context-menu(3)\tGH\t2026-10-29\t/tmp/F.app/x.appex\n\
                    \x20(5 plug-ins)\n";
        let found = parse_plugins(text);
        assert_eq!(
            found[0],
            Served {
                id: "com.example.packwing.context-menu".into(),
                path: "/Applications/Pack Wing.app/Contents/PlugIns/ContextMenu.appex".into(),
                election: Election::On,
            }
        );
        assert_eq!(found[1].election, Election::Off);
        assert_eq!(found[2].election, Election::None);
        assert_eq!(found[2].id, "com.example.third.context-menu");
        assert_eq!(
            found[3].id, "com.example.fourth.context-menu",
            "the superseded left out"
        );
        assert_eq!(found[3].election, Election::None);
        assert_eq!(found.len(), 4);
        assert!(parse_plugins("  (no matches)\n").is_empty());
    }
}
