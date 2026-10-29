//! What macOS says and is told: which Finder extension serves a menu, and
//! that one is there or should be on; the file types of files with each
//! extension, from its type database; which Quick Actions are turned on;
//! and that the Services folder changed.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::{EXTENSION_PATH, MENU_KEY, Served, Types, parse_plugins, xml};
use crate::Error;

const OSASCRIPT: &str = "/usr/bin/osascript";
const DEFAULTS: &str = "/usr/bin/defaults";
const PLUGINKIT: &str = "/usr/bin/pluginkit";
const PLUTIL: &str = "/usr/bin/plutil";
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/\
                          LaunchServices.framework/Support/lsregister";

/// The Finder extension serving the menu `id`, as the system knows it: the
/// one whose `Info.plist` names the menu, with the user's election.
pub(super) fn served(id: &str) -> Option<Served> {
    let out = Command::new(PLUGINKIT)
        .args(["-m", "-v", "-p", "com.apple.FinderSync"])
        .arg(format!("{MENU_KEY}={id}"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    parse_plugins(&String::from_utf8_lossy(&out.stdout))
        .into_iter()
        .next()
}

/// Whether the app bundle `app` holds the extension serving the menu `id`.
pub(super) fn bundled(id: &str, app: &Path) -> bool {
    let info = app.join(EXTENSION_PATH).join("Contents/Info.plist");
    if !info.is_file() {
        return false;
    }
    Command::new(PLUTIL)
        .args(["-extract", MENU_KEY, "raw", "-o", "-"])
        .arg(&info)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim_end() == id)
}

/// Tells the system of the extension serving the menu `id` in the app
/// bundle `app`, when it holds one, as it learns of apps in the
/// Applications folder by itself: LaunchServices first, which PlugInKit
/// needs, then PlugInKit. True when the system then knows it.
pub(super) fn register(id: &str, app: &Path) -> bool {
    if !bundled(id, app) {
        return false;
    }
    let run = |command: &mut Command| {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if !(run(Command::new(LSREGISTER).args(["-f", "-R"]).arg(app))
        && run(Command::new(PLUGINKIT)
            .arg("-a")
            .arg(app.join(EXTENSION_PATH))))
    {
        return false;
    }
    // PlugInKit takes the extension in a moment later.
    for _ in 0..20 {
        if served(id).is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Turns on the extension `id`, as System Settings does when the user does.
pub(super) fn turn_on_extension(id: &str) -> Result<(), Error> {
    let out = Command::new(PLUGINKIT)
        .args(["-e", "use", "-i", id])
        .stdin(Stdio::null())
        .output()
        .map_err(|source| Error::Io {
            path: PathBuf::from(PLUGINKIT),
            source,
        })?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Io {
            path: PathBuf::from(PLUGINKIT),
            source: io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        })
    }
}

/// For each argument, `.` and an extension, a line: the extension, and the
/// types a file with it may have, all separated by tabs. The first is what
/// a file so named is: a type an app or the system declares for it, or one
/// made up for the extension (`dyn.…`) when none does; then any other
/// declared for it, as packages' are. UTType is macOS 11's.
const LOOKUP: &str = r#"
ObjC.import("Foundation");
$.NSBundle.bundleWithPath("/System/Library/Frameworks/UniformTypeIdentifiers.framework").load;
function run(argv) {
  const UTType = $.NSClassFromString("UTType");
  return argv.map(arg => {
    const ext = arg.slice(1);
    const ids = [];
    const add = t => {
      if (!t.isNil() && !ids.includes(t.identifier.js)) ids.push(t.identifier.js);
    };
    add(UTType.typeWithFilenameExtension(ext));
    const all = UTType.typesWithTagTagClassConformingToType(ext, "public.filename-extension", $());
    for (let i = 0; i < all.count; i++) {
      const t = all.objectAtIndex(i);
      if (!t.dynamic) add(t);
    }
    return [ext].concat(ids).join("\t");
  }).join("\n");
}
"#;

/// The file types of files with each of `extensions`, as the system knows
/// them now, by JavaScript for Automation: the crate calls no Objective-C
/// itself.
pub(super) fn types(extensions: &[&str]) -> Result<Types, Error> {
    if extensions.is_empty() {
        return Ok(Types::new());
    }
    let failed = |source: io::Error| Error::Io {
        path: PathBuf::from(OSASCRIPT),
        source,
    };
    // Each with a dot, so none is taken for an option.
    let out = Command::new(OSASCRIPT)
        .args(["-l", "JavaScript", "-e", LOOKUP])
        .args(extensions.iter().map(|e| format!(".{e}")))
        .stdin(Stdio::null())
        .output()
        .map_err(failed)?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(failed(io::Error::other(format!(
            "looking up file types: {}",
            why.trim()
        ))));
    }
    let mut types = Types::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut fields = line.split('\t');
        if let Some(ext) = fields.next().filter(|e| extensions.contains(e)) {
            types.insert(ext.to_string(), fields.map(str::to_string).collect());
        }
    }
    Ok(types)
}

/// A Quick Action turned on wherever System Settings turns one on: Finder's
/// right-click menu and preview pane, and the Services menu.
const ON: &str = "<dict><key>presentation_modes</key><dict>\
    <key>ContextMenu</key><true/><key>FinderPreview</key><true/>\
    <key>ServicesMenu</key><true/><key>TouchBar</key><false/></dict></dict>";

/// The key the system keeps whether the Quick Action labelled `label` is on
/// under: any workflow's of that label.
fn key(label: &str) -> String {
    format!("(null) - {label} - runWorkflowAsService")
}

/// Turns on the Quick Actions labelled `labels` that the user has not
/// turned on or off yet, in `NSServicesStatus` in the preferences `status`:
/// the system leaves a new one off until the user turns it on, in Quick
/// Actions, Customize…, and writes this when they do. The user's own
/// choice, either way, stays.
pub(super) fn turn_on(labels: &[&str], status: &str) -> Result<(), Error> {
    let run = |args: &[String]| -> Result<String, Error> {
        let failed = |source: io::Error| Error::Io {
            path: PathBuf::from(DEFAULTS),
            source,
        };
        let out = Command::new(DEFAULTS)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(failed)?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr);
            return Err(failed(io::Error::other(why.trim().to_string())));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let there = run(&["export".into(), status.into(), "-".into()])?;
    let mut args: Vec<String> = ["write", status, "NSServicesStatus", "-dict-add"]
        .map(String::from)
        .into();
    for label in labels {
        // As `defaults export` writes keys, and `defaults` reads them.
        let key = xml(&key(label));
        if !there.contains(&format!("<key>{key}</key>")) {
            args.push(format!("<string>{key}</string>"));
            args.push(ON.into());
        }
    }
    if args.len() > 4 {
        run(&args)?;
    }
    Ok(())
}

/// Tells the system that the Services folder changed, as Automator does when
/// it saves a Quick Action, without waiting: it takes seconds.
pub(super) fn refresh() {
    let child = Command::new("/System/Library/CoreServices/pbs")
        .arg("-update")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        std::thread::spawn(move || child.wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_knows_archives() {
        let types = types(&["zip", "gz", "-odd", "context-menu-rs"]).unwrap();
        assert_eq!(types["zip"][0], "public.zip-archive");
        assert_eq!(types["gz"][0], "org.gnu.gnu-zip-archive");
        // Extensions nothing declares get a type made up for them, the one
        // their files have.
        assert!(types["-odd"][0].starts_with("dyn."), "{types:?}");
        assert!(types["context-menu-rs"][0].starts_with("dyn."), "{types:?}");
        assert!(super::types(&[]).unwrap().is_empty());
    }
}
