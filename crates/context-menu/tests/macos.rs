//! The Quick Actions written to a scratch folder, not the Services folder
//! Finder reads, with the file types the system knows; read back as the
//! system reads them, and run as Finder runs them.

#![cfg(target_os = "macos")]
// A test: failing loudly is its job, in the helpers too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use context_menu::macos::{Root, install, is_current, uninstall};
use context_menu::{Count, Item, Menu};

/// A scratch folder, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "context-menu-rs-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    /// The Services folder, and the preferences saying which Quick
    /// Actions are on, `status.plist`, in the scratch folder.
    fn root(&self) -> Root {
        Root {
            services: self.0.join("Services"),
            status: self.0.join("status").display().to_string(),
        }
    }

    /// The preferences, as XML.
    fn status(&self) -> String {
        let out = Command::new("/usr/bin/defaults")
            .args(["export", &self.root().status, "-"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A menu whose items write the paths they get to `out`, one per line.
fn menu(out: &Path) -> Menu {
    let record = format!("printf '%s\\n' \"$@\" > '{}'", out.display());
    let item =
        |id: &str, label: &str| Item::new(id, label, "/bin/sh").args(["-c", record.as_str(), "sh"]);
    Menu::new("context-menu-test", "context-menu test")
        .submenu(true)
        .item(item("archives", "Archives").extensions(["zip", "tar.gz"]))
        .item(
            item("folder", "One folder")
                .folders_only()
                .count(Count::One),
        )
}

/// What the system reads of a bundle's service: `pbs -read_bundle`.
fn service(bundle: &Path) -> String {
    let out = Command::new("/System/Library/CoreServices/pbs")
        .arg("-read_bundle")
        .arg(bundle)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    // pbs logs it, to the standard error.
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn the_menu_goes_in_and_comes_out() {
    let s = Scratch::new("in-out");
    let out = s.0.join("out.txt");
    let menu = menu(&out);
    let root = s.root();
    assert!(!is_current(&menu, &root).unwrap(), "nothing yet");

    let report = install(&menu, &root).unwrap();
    let archives = root.services.join("context-menu-test.archives.workflow");
    let folder = root.services.join("context-menu-test.folder.workflow");
    assert_eq!(report.written, [archives.clone(), folder.clone()]);
    assert!(is_current(&menu, &root).unwrap());

    for bundle in [&archives, &folder] {
        for file in ["Info.plist", "document.wflow"] {
            let path = bundle.join("Contents").join(file);
            let lint = Command::new("/usr/bin/plutil")
                .arg("-lint")
                .arg(&path)
                .output()
                .unwrap();
            assert!(lint.status.success(), "{lint:?}");
        }
    }
    let read = service(&archives);
    for want in [
        "default = Archives;",
        "NSMessage = runWorkflowAsService;",
        "NSApplicationIdentifier = \"com.apple.finder\";",
        "\"public.zip-archive\"",
        "\"org.gnu.gnu-zip-archive\"",
    ] {
        assert!(read.contains(want), "{want} in {read}");
    }
    assert!(service(&folder).contains("\"public.folder\""));

    // Turned on, in Finder's menu: the system leaves new ones off.
    let status = s.status();
    let on = "<key>(null) - Archives - runWorkflowAsService</key>\n\
              \t\t<dict>\n\
              \t\t\t<key>presentation_modes</key>\n\
              \t\t\t<dict>\n\
              \t\t\t\t<key>ContextMenu</key>\n\
              \t\t\t\t<true/>\n";
    assert!(status.contains(on), "{status}");
    assert!(status.contains("<key>(null) - One folder - runWorkflowAsService</key>"));

    // One the user turned off stays off.
    let off = "<dict><key>presentation_modes</key><dict>\
               <key>ContextMenu</key><false/><key>ServicesMenu</key><false/></dict></dict>";
    let write = Command::new("/usr/bin/defaults")
        .args(["write", &root.status, "NSServicesStatus", "-dict-add"])
        .args([
            "<string>(null) - Archives - runWorkflowAsService</string>",
            off,
        ])
        .status()
        .unwrap();
    assert!(write.success());
    install(&menu, &root).unwrap();
    let status = s.status();
    assert!(!status.contains(on), "{status}");
    assert!(status.contains("<key>(null) - One folder - runWorkflowAsService</key>"));

    // Run as Finder runs it: the paths, as arguments, in one run.
    let paths = ["/tmp/a b.zip", "/tmp/it's \"x\".tar.gz", "/tmp/$HOME.zip"];
    let run = Command::new("/usr/bin/automator")
        .arg("-i")
        .arg(paths.join("\n"))
        .arg(&archives)
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    // The program is started, not waited for.
    let mut got = String::new();
    for _ in 0..50 {
        got = std::fs::read_to_string(&out).unwrap_or_default();
        if got.lines().count() == paths.len() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(got.lines().collect::<Vec<_>>(), paths);

    // Changed: not current, and installing again drops what went.
    let mut smaller = menu.clone();
    smaller.items.pop();
    assert!(!is_current(&smaller, &root).unwrap());
    let report = install(&smaller, &root).unwrap();
    assert_eq!(report.removed, std::slice::from_ref(&folder));
    assert!(!folder.exists());
    assert!(is_current(&smaller, &root).unwrap());

    let report = uninstall("context-menu-test", &root).unwrap();
    assert_eq!(report.removed, std::slice::from_ref(&archives));
    assert!(!archives.exists());
    assert!(
        uninstall("context-menu-test", &root)
            .unwrap()
            .removed
            .is_empty()
    );
}
