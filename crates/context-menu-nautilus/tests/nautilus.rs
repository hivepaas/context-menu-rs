//! The extension, loaded as Nautilus loads it, with Nautilus's own library:
//! a C harness (`tests/harness.c`) asks it what it shows for files, and
//! chooses an item. It needs a C compiler, GLib's headers (`pkg-config
//! gobject-2.0 gio-2.0`) and `libnautilus-extension.so.4`; without them the
//! tests are skipped, unless `CONTEXT_MENU_NAUTILUS_REQUIRED` is set, as CI
//! sets it.

#![cfg(target_os = "linux")]
// A test: failing loudly is its job, in the helpers too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use context_menu::{Count, Item, Menu, linux};

/// Nautilus's library, where a distribution puts it.
fn nautilus_library() -> Option<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/lib64"), PathBuf::from("/usr/lib")];
    if let Ok(entries) = std::fs::read_dir("/usr/lib") {
        dirs.extend(
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.to_string_lossy().contains("-linux-")),
        );
    }
    dirs.into_iter()
        .map(|d| d.join("libnautilus-extension.so.4"))
        .find(|p| p.exists())
}

/// Whether to go on without what the test needs: no, when CI asks for it.
fn missing(what: &str) {
    if std::env::var_os("CONTEXT_MENU_NAUTILUS_REQUIRED").is_some() {
        panic!("{what}, which CONTEXT_MENU_NAUTILUS_REQUIRED requires");
    }
    eprintln!("skipped: {what}");
}

/// A fresh scratch folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "context-menu-nautilus-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The harness, compiled into `dir`; None when it cannot be.
fn harness(dir: &Path) -> Option<PathBuf> {
    let flags = Command::new("pkg-config")
        .args(["--cflags", "--libs", "gobject-2.0", "gio-2.0"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let flags = String::from_utf8(flags.stdout).unwrap();
    let out = dir.join("harness");
    let built = Command::new("cc")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/harness.c"))
        .arg("-o")
        .arg(&out)
        .args(flags.split_whitespace())
        .arg("-ldl")
        .status()
        .ok()?;
    assert!(built.success(), "the harness does not compile");
    Some(out)
}

/// What these tests need, or None when it is missing.
fn setup(name: &str) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let Some(library) = nautilus_library() else {
        missing("no libnautilus-extension.so.4");
        return None;
    };
    let dir = scratch(name);
    let Some(harness) = harness(&dir) else {
        missing("no C compiler or GLib headers");
        return None;
    };
    Some((dir, library, harness))
}

/// The extension cargo built, in the folder above the tests' binary.
fn built() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let ext = exe
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("libcontext_menu_nautilus.so");
    assert!(ext.exists(), "{} is not built", ext.display());
    ext
}

/// A program that writes its arguments to `$OUT`, one per line, whole.
fn recorder(dir: &Path) -> PathBuf {
    let path = dir.join("record");
    std::fs::write(
        &path,
        "#!/bin/sh\nfor a; do printf '%s\\n' \"$a\"; done > \"$OUT.tmp\" && mv \"$OUT.tmp\" \"$OUT\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn the_extension_shows_the_menu_in_nautilus() {
    let Some((dir, library, harness)) = setup("shows") else {
        return;
    };
    // As the app's package installs it: renamed for its menu.
    let extensions = dir.join("extensions-4");
    std::fs::create_dir_all(&extensions).unwrap();
    let ext = extensions.join(linux::native_name("photo-kit"));
    std::fs::copy(built(), &ext).unwrap();

    let program = recorder(&dir);
    let menu = Menu::new("photo-kit", "Photo Kit")
        .submenu(true)
        .item(
            Item::new("resize", "Resize", &program)
                .args(["--resize", "it's"])
                .extensions(["jpg", "png"]),
        )
        .item(
            Item::new("folder", "Pack folder", &program)
                .folders_only()
                .count(Count::One),
        )
        .item(Item::new("two", "Compare", &program).count(Count::Many));
    let dirs = linux::Dirs {
        data_home: dir.join("data"),
        data_dirs: Vec::new(),
        nautilus_extensions: vec![extensions],
    };
    let report = linux::install(&menu, &dirs).unwrap();
    // With the native extension there, Nautilus gets no scripts and no
    // Python extension.
    for p in &report.written {
        let p = p.strip_prefix(&dirs.data_home).unwrap();
        assert!(
            !p.starts_with("nautilus") && !p.starts_with("nautilus-python"),
            "{p:?}"
        );
    }

    let out = dir.join("out");
    let run = Command::new(&harness)
        // A GLib critical, such as a reference dropped twice, aborts.
        .env("G_DEBUG", "fatal-criticals")
        .arg(&library)
        .arg(&ext)
        .args([
            "a.JPG",
            "a.jpg,b.png",
            "photos/",
            "photos/,a.jpg",
            "notes.txt",
            "sftp:c.jpg",
        ])
        .env("XDG_DATA_HOME", &dirs.data_home)
        .env("OUT", &out)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "types: 1\n\
         a.JPG: Photo Kit > Resize\n\
         a.jpg,b.png: Photo Kit > Resize, Compare\n\
         photos/: Photo Kit > Pack folder\n\
         photos/,a.jpg:\n\
         notes.txt:\n\
         sftp:c.jpg:\n"
    );
    // Choosing the first item ran its program with the path.
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "--resize\nit's\n/home/u/a.JPG\n"
    );
}

#[test]
fn a_changed_menu_shows_at_once() {
    let Some((dir, library, harness)) = setup("changed") else {
        return;
    };
    let extensions = dir.join("extensions-4");
    std::fs::create_dir_all(&extensions).unwrap();
    let ext = extensions.join(linux::native_name("m"));
    std::fs::copy(built(), &ext).unwrap();
    let dirs = linux::Dirs {
        data_home: dir.join("data"),
        data_dirs: Vec::new(),
        nautilus_extensions: vec![extensions],
    };
    let show = |menu: &Menu| {
        linux::install(menu, &dirs).unwrap();
        let run = Command::new(&harness)
            // A GLib critical, such as a reference dropped twice, aborts.
            .env("G_DEBUG", "fatal-criticals")
            .arg(&library)
            .arg(&ext)
            .arg("a.txt")
            .env("XDG_DATA_HOME", &dirs.data_home)
            .output()
            .unwrap();
        String::from_utf8_lossy(&run.stdout).into_owned()
    };
    let one = Menu::new("m", "M").item(Item::new("a", "First", "/bin/true"));
    assert_eq!(show(&one), "types: 1\na.txt: First\n");
    let two = one.item(Item::new("b", "Second", "/bin/true"));
    assert_eq!(show(&two), "types: 1\na.txt: First, Second\n");
}

#[test]
fn a_library_not_renamed_serves_no_menu() {
    let Some((_, library, harness)) = setup("unnamed") else {
        return;
    };
    let run = Command::new(&harness)
        // A GLib critical, such as a reference dropped twice, aborts.
        .env("G_DEBUG", "fatal-criticals")
        .arg(&library)
        .arg(built())
        .arg("a.txt")
        .output()
        .unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "types: 0\n");
    assert!(String::from_utf8_lossy(&run.stderr).contains("is not named libcontext-menu-"));
}
