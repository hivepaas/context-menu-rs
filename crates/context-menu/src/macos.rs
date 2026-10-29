//! macOS: Finder's Quick Actions, a workflow bundle per item in the user's
//! Services folder, `~/Library/Services`, with no rights beyond the user's.
//! See `docs/macos.md`.
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
}

impl Root {
    /// The current user's.
    pub fn current() -> Result<Root, Error> {
        let home = std::env::home_dir()
            .filter(|h| h.is_absolute())
            .ok_or(Error::NoHome)?;
        Ok(Root {
            services: home.join("Library/Services"),
            status: "pbs".into(),
        })
    }
}

/// The file types of files with each extension, as the system names them:
/// by the extension, the last of an item's (`gz` for `tar.gz`), and for
/// each, the types a file so named may have.
pub(crate) type Types = BTreeMap<String, Vec<String>>;

/// A Quick Action: the bundle's name, and its two files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bundle {
    pub name: String,
    pub label: String,
    pub info: String,
    pub document: String,
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
        bundles.push(Bundle {
            name: format!("{}.{}.workflow", menu.id, item.id),
            label: item.label.clone(),
            info: info(menu, item, &file_types),
            document: document(item),
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
fn info(menu: &Menu, item: &Item, file_types: &[String]) -> String {
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
			<string>NSActionTemplate</string>
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

/// Installs `menu` under `root`, with the file types `types`, in place of an
/// earlier install: the bundles that one wrote and this one does not are
/// removed.
pub(crate) fn install_with(menu: &Menu, root: &Root, types: &Types) -> Result<Report, Error> {
    menu.validate()?;
    let (bundles, notes) = plan(menu, types);
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
    Ok(report)
}

/// Takes the bundles of the menu `id` away from under `root`.
pub(crate) fn uninstall_from(id: &str, root: &Root) -> Result<Report, Error> {
    let mut report = Report::default();
    for path in existing(id, root)? {
        remove(&path).map_err(|e| Error::io(&path, e))?;
        report.removed.push(path);
    }
    Ok(report)
}

/// Whether the bundles under `root` are those [`install_with`] would write
/// for `menu` and `types`, and no others of the menu's.
pub(crate) fn is_current_with(menu: &Menu, root: &Root, types: &Types) -> Result<bool, Error> {
    menu.validate()?;
    let (bundles, _) = plan(menu, types);
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
        read("Info.plist").as_deref() == Some(b.info.as_str())
            && read("document.wflow").as_deref() == Some(b.document.as_str())
    }))
}

#[cfg(target_os = "macos")]
mod system;

/// Installs `menu` under `root`, in place of an earlier install, with the
/// file types the system knows for its extensions now, and turns on its
/// Quick Actions the user has not turned on or off yet.
#[cfg(target_os = "macos")]
pub fn install(menu: &Menu, root: &Root) -> Result<Report, Error> {
    menu.validate()?;
    let types = system::types(&extensions(menu))?;
    let mut report = install_with(menu, root, &types)?;
    let (bundles, _) = plan(menu, &types);
    let labels: Vec<&str> = bundles.iter().map(|b| b.label.as_str()).collect();
    if let Err(e) = system::turn_on(&labels, &root.status) {
        report.notes.push(format!(
            "Finder: the items could not be turned on ({e}): Customize…, in its Quick \
             Actions submenu, turns them on."
        ));
    }
    system::refresh();
    Ok(report)
}

/// Takes the menu `id` away from under `root`.
#[cfg(target_os = "macos")]
pub fn uninstall(id: &str, root: &Root) -> Result<Report, Error> {
    let report = uninstall_from(id, root)?;
    if !report.removed.is_empty() {
        system::refresh();
    }
    Ok(report)
}

/// Whether the bundles under `root` hold `menu` as [`install`] writes it
/// now, and nothing else of the menu's: false too when the file types the
/// system knows for its extensions changed, as when an app declaring one
/// is installed.
#[cfg(target_os = "macos")]
pub fn is_current(menu: &Menu, root: &Root) -> Result<bool, Error> {
    menu.validate()?;
    is_current_with(menu, root, &system::types(&extensions(menu))?)
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
        let report = install_with(&menu(true), &r, &types()).unwrap();
        assert_eq!(report.written.len(), 3);
        assert!(report.removed.is_empty());
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
        let report = install_with(&smaller, &r, &types()).unwrap();
        assert_eq!(
            report.removed,
            [r.services.join("packwing.compare.workflow")]
        );
        assert_eq!(report.written.len(), 2);
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
        assert_eq!(report.removed.len(), 2);
        assert!(others.iter().all(|o| o.exists()));
        assert!(uninstall_from("packwing", &r).unwrap().removed.is_empty());
    }

    #[test]
    fn current_until_the_menu_or_the_types_change() {
        let r = root("current");
        assert!(
            !is_current_with(&menu(true), &r, &types()).unwrap(),
            "nothing installed"
        );
        install_with(&menu(true), &r, &types()).unwrap();
        assert!(is_current_with(&menu(true), &r, &types()).unwrap());
        let mut renamed = menu(true);
        renamed.items[0].label = "Extract".into();
        assert!(
            !is_current_with(&renamed, &r, &types()).unwrap(),
            "the menu changed"
        );

        // An app declares 7z's type: the filter must change.
        let mut declared = types();
        declared.insert("7z".into(), vec!["com.example.7z".into()]);
        assert!(!is_current_with(&menu(true), &r, &declared).unwrap());

        // A bundle changed by hand, or one gone.
        let info = r
            .services
            .join("packwing.compress.workflow/Contents/Info.plist");
        fs::write(&info, "changed").unwrap();
        assert!(!is_current_with(&menu(true), &r, &types()).unwrap());
        install_with(&menu(true), &r, &types()).unwrap();
        fs::remove_dir_all(r.services.join("packwing.compare.workflow")).unwrap();
        assert!(!is_current_with(&menu(true), &r, &types()).unwrap());
    }
}
