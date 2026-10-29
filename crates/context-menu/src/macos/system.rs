//! What macOS says and is told: the file types of files with each
//! extension, from its type database; which Quick Actions are turned on;
//! and that the Services folder changed.

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::{Types, xml};
use crate::Error;

const OSASCRIPT: &str = "/usr/bin/osascript";
const DEFAULTS: &str = "/usr/bin/defaults";

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
