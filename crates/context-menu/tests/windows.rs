//! The classic menu written to a scratch key of the registry, not the one
//! Explorer reads, and read back.

#![cfg(windows)]
// A test: failing loudly is its job, in the helpers too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use context_menu::open_with::Handler;
use context_menu::windows::{Root, install, is_current, open_with, uninstall};
use context_menu::{Count, Item, Menu};
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

/// A scratch root, removed when the test ends.
struct Scratch {
    root: Root,
    key: String,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let key = format!(
            r"Software\context-menu-rs-test-{}-{name}",
            std::process::id()
        );
        let data_home = std::env::temp_dir().join(format!(
            "context-menu-rs-test-{}-{name}",
            std::process::id()
        ));
        Scratch {
            root: Root {
                classes: format!(r"{key}\Classes"),
                data_home,
                // Where packages are listed, both under the scratch key:
                // none until a test adds one.
                class_index: format!(r"HKEY_CURRENT_USER\{key}\ClassIndex"),
                user_packages: format!(r"HKEY_CURRENT_USER\{key}\Packages"),
            },
            key,
        }
    }

    fn classes(&self) -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(&self.root.classes)
            .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.key);
        let _ = std::fs::remove_dir_all(&self.root.data_home);
    }
}

const APP: &str = r"C:\Program Files\Photo Kit\photokit.exe";

fn menu() -> Menu {
    Menu::new("photo-kit", "Photo Kit")
        .submenu(true)
        .item(
            Item::new("resize", "Resize", APP)
                .args(["--resize", "it's"])
                .extensions(["jpg", "png"]),
        )
        .item(
            Item::new("folder", "Pack folder", APP)
                .folders_only()
                .count(Count::One),
        )
}

#[test]
fn the_menu_goes_in_and_comes_out() {
    let s = Scratch::new("in-out");
    assert!(!is_current(&menu(), &s.root).unwrap(), "nothing yet");

    let r = install(&menu(), &s.root).unwrap();
    let names: Vec<String> = r.written.iter().map(|p| p.display().to_string()).collect();
    assert!(
        names.contains(&format!(
            r"HKEY_CURRENT_USER\{}\SystemFileAssociations\.jpg\shell\photo-kit",
            s.root.classes
        )),
        "{names:?}"
    );
    assert!(is_current(&menu(), &s.root).unwrap());

    let c = s.classes();
    let verb = c
        .open_subkey(r"SystemFileAssociations\.png\shell\photo-kit")
        .unwrap();
    assert_eq!(verb.get_value::<String, _>("MUIVerb").unwrap(), "Photo Kit");
    let command = c
        .open_subkey(r"SystemFileAssociations\.png\shell\photo-kit\shell\00.resize\command")
        .unwrap();
    assert_eq!(
        command.get_value::<String, _>("").unwrap(),
        r#""C:\Program Files\Photo Kit\photokit.exe" "--resize" "it's" "%1""#
    );
    let folder = c
        .open_subkey(r"Directory\shell\photo-kit\shell\01.folder")
        .unwrap();
    assert_eq!(
        folder.get_value::<String, _>("MultiSelectModel").unwrap(),
        "Single"
    );
    let definition = std::fs::read_to_string(context_menu::windows::definition_path(
        "photo-kit",
        &s.root.data_home,
    ))
    .unwrap();
    assert_eq!(Menu::from_definition(&definition).unwrap(), menu());

    // Changed: not current, and installing again drops what went.
    let mut smaller = menu();
    smaller.items.pop();
    assert!(!is_current(&smaller, &s.root).unwrap());
    let r = install(&smaller, &s.root).unwrap();
    assert_eq!(r.removed.len(), 1, "{:?}", r.removed);
    assert!(c.open_subkey(r"Directory\shell\photo-kit").is_err());
    assert!(is_current(&smaller, &s.root).unwrap());

    // A key a user changed by hand.
    c.open_subkey_with_flags(
        r"SystemFileAssociations\.jpg\shell\photo-kit",
        winreg::enums::KEY_WRITE,
    )
    .unwrap()
    .set_value("MUIVerb", &"Mine")
    .unwrap();
    assert!(!is_current(&smaller, &s.root).unwrap());

    let r = uninstall("photo-kit", &s.root).unwrap();
    assert_eq!(
        r.removed.len(),
        3,
        "two verbs and the definition: {:?}",
        r.removed
    );
    assert!(
        c.open_subkey(r"SystemFileAssociations\.jpg\shell\photo-kit")
            .is_err()
    );
    assert!(uninstall("photo-kit", &s.root).unwrap().removed.is_empty());
}

#[test]
fn other_verbs_stay() {
    let s = Scratch::new("others");
    install(&menu(), &s.root).unwrap();
    let c = s.classes();
    // Another app's verb, and one of the same name with no mark.
    for path in [
        r"SystemFileAssociations\.jpg\shell\gimp",
        r"Directory\shell\photo-kit-x",
    ] {
        c.create_subkey(path)
            .unwrap()
            .0
            .set_value("MUIVerb", &"Theirs")
            .unwrap();
    }
    let unmarked = r"*\shell\photo-kit";
    c.create_subkey(unmarked).unwrap();
    uninstall("photo-kit", &s.root).unwrap();
    for path in [
        r"SystemFileAssociations\.jpg\shell\gimp",
        r"Directory\shell\photo-kit-x",
        unmarked,
    ] {
        assert!(c.open_subkey(path).is_ok(), "{path}");
    }
}

#[test]
fn a_package_serving_the_menu_takes_the_classic_one_too() {
    let s = Scratch::new("package");
    install(&menu(), &s.root).unwrap();
    let c = s.classes();
    assert!(c.open_subkey(r"Directory\shell\photo-kit").is_ok());

    // A package declaring the menu's class, but another user's: no change.
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let name = "PhotoKit_1.0.0.0_x64__abcdefghjkmnp";
    let class = format!(
        r"{}\ClassIndex\{{{}}}\{name}",
        s.key,
        context_menu::windows::clsid("photo-kit")
    );
    hkcu.create_subkey(&class).unwrap();
    assert!(is_current(&menu(), &s.root).unwrap());

    // Registered for this user: the classic verbs go, the definition stays.
    hkcu.create_subkey(format!(r"{}\Packages\{name}", s.key))
        .unwrap();
    assert!(!is_current(&menu(), &s.root).unwrap());
    let r = install(&menu(), &s.root).unwrap();
    assert_eq!(r.removed.len(), 3, "{:?}", r.removed);
    assert_eq!(r.written.len(), 1, "the definition: {:?}", r.written);
    assert!(r.notes[0].contains(name), "{:?}", r.notes);
    assert!(c.open_subkey(r"Directory\shell\photo-kit").is_err());
    assert!(is_current(&menu(), &s.root).unwrap());

    // The package removed: the classic menu again.
    hkcu.delete_subkey_all(format!(r"{}\Packages\{name}", s.key))
        .unwrap();
    assert!(!is_current(&menu(), &s.root).unwrap());
    install(&menu(), &s.root).unwrap();
    assert!(c.open_subkey(r"Directory\shell\photo-kit").is_ok());
}

fn handler() -> Handler {
    Handler::new("photo-kit", "Photo Kit", APP)
        .arg("--open")
        .icon(r"C:\Program Files\Photo Kit\photokit.exe,0")
        .extensions(["jpg", "png"])
}

#[test]
fn the_handler_goes_in_and_comes_out() {
    let s = Scratch::new("open-with");
    assert!(!open_with::is_current(&handler(), &s.root).unwrap());
    // Another app's, in the same list.
    RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(format!(r"{}\.jpg\OpenWithProgids", s.root.classes))
        .unwrap()
        .0
        .set_value("Theirs.jpg", &String::new())
        .unwrap();

    let r = open_with::install(&handler(), &s.root).unwrap();
    let c = s.classes();
    let name = |path: &str| format!(r"HKEY_CURRENT_USER\{}\{path}", s.root.classes);
    let names: Vec<String> = r.written.iter().map(|p| p.display().to_string()).collect();
    assert_eq!(
        names,
        [
            name("ContextMenuRs.photo-kit"),
            name(r".jpg\OpenWithProgids\ContextMenuRs.photo-kit"),
            name(r".png\OpenWithProgids\ContextMenuRs.photo-kit"),
            format!(
                r"HKEY_CURRENT_USER\{}\RegisteredApplications\ContextMenuRs.photo-kit",
                s.key
            ),
        ]
    );
    assert!(open_with::is_current(&handler(), &s.root).unwrap());
    let command = c
        .open_subkey(r"ContextMenuRs.photo-kit\shell\open\command")
        .unwrap();
    assert_eq!(
        command.get_value::<String, _>("").unwrap(),
        r#""C:\Program Files\Photo Kit\photokit.exe" "--open" "%1""#
    );
    let registered = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!(r"{}\RegisteredApplications", s.key))
        .unwrap();
    assert_eq!(
        registered
            .get_value::<String, _>("ContextMenuRs.photo-kit")
            .unwrap(),
        format!(r"{}\ContextMenuRs.photo-kit\Capabilities", s.root.classes)
    );

    // For fewer files: out of the list of the one it is no longer for, and
    // the list and the extension's key with it, as nothing else is there.
    let fewer = Handler {
        extensions: vec!["jpg".into()],
        ..handler()
    };
    assert!(!open_with::is_current(&fewer, &s.root).unwrap());
    let r = open_with::install(&fewer, &s.root).unwrap();
    assert_eq!(r.removed.len(), 1, "{:?}", r.removed);
    assert!(c.open_subkey(".png").is_err());
    assert!(open_with::is_current(&fewer, &s.root).unwrap());

    // Changed by hand.
    c.open_subkey_with_flags(
        r"ContextMenuRs.photo-kit\shell\open",
        winreg::enums::KEY_WRITE,
    )
    .unwrap()
    .set_value("FriendlyAppName", &"Mine")
    .unwrap();
    assert!(!open_with::is_current(&fewer, &s.root).unwrap());

    let r = open_with::uninstall("photo-kit", &s.root).unwrap();
    assert_eq!(r.removed.len(), 3, "{:?}", r.removed);
    assert!(c.open_subkey("ContextMenuRs.photo-kit").is_err());
    assert!(registered.get_raw_value("ContextMenuRs.photo-kit").is_err());
    // The other app's stays.
    let jpg = c.open_subkey(r".jpg\OpenWithProgids").unwrap();
    assert!(jpg.get_raw_value("Theirs.jpg").is_ok());
    assert!(jpg.get_raw_value("ContextMenuRs.photo-kit").is_err());
    let again = open_with::uninstall("photo-kit", &s.root).unwrap();
    assert!(again.removed.is_empty(), "{:?}", again.removed);
}
