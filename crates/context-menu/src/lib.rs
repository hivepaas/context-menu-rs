//! Put your app in the right-click menu of the system's file manager.
//!
//! Describe the menu once, as a [`Menu`] of [`Item`]s, and [`install`] it for
//! the current user; [`uninstall`] takes it away again. Each item runs a
//! program with the arguments given and then the paths of the files and
//! folders chosen, all in one run.
//!
//! ```no_run
//! use context_menu::{Count, Item, Menu};
//!
//! let app = "/usr/bin/packwing";
//! let menu = Menu::new("packwing", "packwing")
//!     .submenu(true)
//!     .item(
//!         Item::new("extract-here", "Extract here", app)
//!             .args(["--extract-here"])
//!             .extensions(["zip", "7z", "rar", "tar.zst"]),
//!     )
//!     .item(
//!         Item::new("compress", "Compress…", app)
//!             .args(["--compress"])
//!             .folders(true),
//!     )
//!     .item(
//!         Item::new("compare", "Compare the two", app)
//!             .args(["--compare"])
//!             .count(Count::Many),
//!     );
//! // When the app starts: again only when the menu, or what the system
//! // offers, changed.
//! if !context_menu::is_current(&menu)? {
//!     let report = context_menu::install(&menu)?;
//!     for note in &report.notes {
//!         println!("{note}");
//!     }
//! }
//! # Ok::<(), context_menu::Error>(())
//! ```
//!
//! Where each file manager keeps its menu, and what each can and cannot do,
//! is in `docs/linux.md`; Nautilus's native extension, which an app's
//! package installs, in `docs/nautilus.md`; Windows's classic menu, and the
//! native extension an app's package declares for Windows 11's own, in
//! `docs/windows.md`. macOS comes next; until then [`install`] returns
//! [`Error::Unsupported`] there.

use std::fmt;
use std::path::PathBuf;

mod definition;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod windows;

/// An app's items in the right-click menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// Names the menu where the system keeps it: file names, and later
    /// registry keys. Lower-case ASCII letters, digits and `-`.
    pub id: String,
    /// The submenu's label, when the items are grouped under one.
    pub title: String,
    /// The submenu's icon: see [`Item::icon`].
    pub icon: Option<String>,
    /// Groups the items under one entry labelled [`title`](Self::title).
    pub submenu: bool,
    /// The entries, in the order the menu shows them.
    pub items: Vec<Item>,
}

/// One entry in the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Unique in its menu; the same characters as [`Menu::id`].
    pub id: String,
    /// What the menu says.
    pub label: String,
    /// On Linux, an icon theme's name (such as `package-x-generic`) or an
    /// absolute path to an image. Some file managers show none.
    pub icon: Option<String>,
    /// What the item shows for.
    pub applies_to: AppliesTo,
    /// The program to run, as an absolute path.
    pub program: PathBuf,
    /// Its arguments, before the paths chosen.
    pub args: Vec<String>,
}

/// What an item shows for: all of the files and folders chosen must match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliesTo {
    /// Which files.
    pub files: Files,
    /// Folders too.
    pub folders: bool,
    /// How many may be chosen.
    pub count: Count,
}

/// A file or folder chosen, as a file manager tells it: what
/// [`AppliesTo::accepts`] decides by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chosen<'a> {
    /// Its name, without the folder it is in.
    pub name: &'a str,
    /// It is a folder.
    pub folder: bool,
}

impl AppliesTo {
    /// Whether an item applying to this shows for `chosen`: as many as it
    /// takes, and each one a file or a folder it applies to. Names match an
    /// extension at their end, in any case: `a.tar.gz` has `gz` and `tar.gz`.
    pub fn accepts(&self, chosen: &[Chosen<'_>]) -> bool {
        let count = match self.count {
            Count::One => chosen.len() == 1,
            Count::Many => chosen.len() >= 2,
            Count::Any => !chosen.is_empty(),
        };
        count
            && chosen.iter().all(|c| {
                if c.folder {
                    return self.folders;
                }
                match &self.files {
                    Files::None => false,
                    Files::Any => true,
                    Files::Extensions(list) => {
                        let name = c.name.to_lowercase();
                        list.iter().any(|ext| {
                            name.len() > ext.len() + 1
                                && name.ends_with(ext.as_str())
                                && name.as_bytes()[name.len() - ext.len() - 1] == b'.'
                        })
                    }
                }
            })
    }
}

/// Which files an item shows for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Files {
    /// None: only folders, with [`AppliesTo::folders`].
    None,
    /// Every file.
    Any,
    /// Those whose names end in one of these, without the dot and in lower
    /// case: `zip`, `tar.gz`.
    Extensions(Vec<String>),
}

/// How many files and folders may be chosen for an item to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Count {
    /// Exactly one.
    One,
    /// Two or more.
    Many,
    /// One or more.
    #[default]
    Any,
}

impl Menu {
    /// A menu with no items yet, not grouped in a submenu.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Menu {
            id: id.into(),
            title: title.into(),
            icon: None,
            submenu: false,
            items: Vec::new(),
        }
    }

    /// The submenu's icon: see [`Item::icon`].
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Groups the items under one entry, labelled with the title.
    pub fn submenu(mut self, submenu: bool) -> Self {
        self.submenu = submenu;
        self
    }

    /// Adds an entry, after those already there.
    pub fn item(mut self, item: Item) -> Self {
        self.items.push(item);
        self
    }

    /// Checks everything [`install`] needs: ids, labels, the program and
    /// what each item applies to.
    pub fn validate(&self) -> Result<(), Error> {
        check_id("the menu's id", &self.id)?;
        check_label("the menu's title", &self.title)?;
        if self.items.is_empty() {
            return Err(Error::Invalid("the menu has no items".into()));
        }
        for (i, item) in self.items.iter().enumerate() {
            item.validate()?;
            if self.items[..i].iter().any(|other| other.id == item.id) {
                return Err(Error::Invalid(format!(
                    "two items have the id {:?}",
                    item.id
                )));
            }
        }
        Ok(())
    }
}

impl Item {
    /// An item for any one or more files, not folders, running `program`
    /// with no arguments before the paths.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        program: impl Into<PathBuf>,
    ) -> Self {
        Item {
            id: id.into(),
            label: label.into(),
            icon: None,
            applies_to: AppliesTo {
                files: Files::Any,
                folders: false,
                count: Count::Any,
            },
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// The entry's icon: see [`Item::icon`](struct.Item.html#structfield.icon).
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Adds an argument, before the paths.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Adds arguments, before the paths.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Only files with these extensions: `zip` or `.zip`, any case.
    pub fn extensions(mut self, extensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let list = extensions
            .into_iter()
            .map(|e| e.as_ref().trim_start_matches('.').to_lowercase())
            .collect();
        self.applies_to.files = Files::Extensions(list);
        self
    }

    /// Only folders: no files.
    pub fn folders_only(mut self) -> Self {
        self.applies_to.files = Files::None;
        self.applies_to.folders = true;
        self
    }

    /// Folders as well as the files it shows for.
    pub fn folders(mut self, folders: bool) -> Self {
        self.applies_to.folders = folders;
        self
    }

    /// How many files and folders may be chosen.
    pub fn count(mut self, count: Count) -> Self {
        self.applies_to.count = count;
        self
    }

    fn validate(&self) -> Result<(), Error> {
        check_id("an item's id", &self.id)?;
        check_label(&format!("the label of {:?}", self.id), &self.label)?;
        if !self.program.is_absolute() {
            return Err(Error::Invalid(format!(
                "the program of {:?} is not an absolute path: {}",
                self.id,
                self.program.display()
            )));
        }
        for arg in &self.args {
            if arg.contains(['\0', '\n', '\r']) {
                return Err(Error::Invalid(format!(
                    "an argument of {:?} holds a NUL or a line break",
                    self.id
                )));
            }
        }
        match &self.applies_to.files {
            Files::None if !self.applies_to.folders => {
                return Err(Error::Invalid(format!(
                    "{:?} applies to neither files nor folders",
                    self.id
                )));
            }
            Files::Extensions(list) => {
                if list.is_empty() {
                    return Err(Error::Invalid(format!("{:?} has no extensions", self.id)));
                }
                for ext in list {
                    let ok = !ext.is_empty()
                        && !ext.starts_with('.')
                        && !ext.ends_with('.')
                        && ext.chars().all(|c| {
                            c.is_alphanumeric() && !c.is_uppercase() || "._-+".contains(c)
                        });
                    if !ok {
                        return Err(Error::Invalid(format!(
                            "{:?} has an extension that cannot be matched: {ext:?}",
                            self.id
                        )));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Whether `id` can name a menu or an item: 1 to 64 lower-case ASCII
/// letters, digits and `-`, neither first nor last. So it fits a file name
/// or a registry key as it is, and `.` can join a menu's and an item's.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

fn check_id(what: &str, id: &str) -> Result<(), Error> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{what} must be 1 to 64 lower-case letters, digits and inner '-': {id:?}"
        )))
    }
}

/// A label: some text, on one line.
fn check_label(what: &str, label: &str) -> Result<(), Error> {
    if label.trim().is_empty() || label.chars().any(char::is_control) || label.len() > 200 {
        Err(Error::Invalid(format!(
            "{what} must be one line of text, up to 200 bytes: {label:?}"
        )))
    } else {
        Ok(())
    }
}

/// What [`install`] or [`uninstall`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The files written, and on Windows the registry keys, as
    /// `HKEY_CURRENT_USER\Software\Classes\…`.
    pub written: Vec<PathBuf>,
    /// The files or keys taken away: an earlier install's.
    pub removed: Vec<PathBuf>,
    /// What the user may need to know or do, such as restarting a file
    /// manager, in English.
    pub notes: Vec<String>,
}

/// Why the menu could not be installed or removed.
#[derive(Debug)]
pub enum Error {
    /// The menu breaks a rule of [`Menu::validate`].
    Invalid(String),
    /// Where the menu goes could not be found: no home folder.
    NoHome,
    /// A file could not be read, written or removed.
    Io {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// Not on this system yet.
    Unsupported,
}

impl Error {
    #[cfg(target_os = "linux")]
    fn io(path: &std::path::Path, source: std::io::Error) -> Self {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(why) => write!(f, "invalid menu: {why}"),
            Error::NoHome => f.write_str("no home folder to install the menu in"),
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Unsupported => f.write_str("right-click menus are not supported here yet"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Installs `menu` for the current user, in place of an earlier install of
/// the same menu.
pub fn install(menu: &Menu) -> Result<Report, Error> {
    menu.validate()?;
    #[cfg(target_os = "linux")]
    {
        linux::install(menu, &linux::Dirs::current()?)
    }
    #[cfg(windows)]
    {
        windows::install(menu, &windows::Root::current()?)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(Error::Unsupported)
    }
}

/// Whether `menu` is installed for the current user as [`install`] would
/// install it now: false when it is not, when it changed, or when what the
/// system offers did (such as nautilus-python, added or removed since).
/// Cheap enough to call when the app starts, and [`install`] when false.
pub fn is_current(menu: &Menu) -> Result<bool, Error> {
    menu.validate()?;
    #[cfg(target_os = "linux")]
    {
        linux::is_current(menu, &linux::Dirs::current()?)
    }
    #[cfg(windows)]
    {
        windows::is_current(menu, &windows::Root::current()?)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(Error::Unsupported)
    }
}

/// Takes the menu `id` away for the current user. Nothing installed is not
/// an error.
pub fn uninstall(id: &str) -> Result<Report, Error> {
    check_id("the menu's id", id)?;
    #[cfg(target_os = "linux")]
    {
        linux::uninstall(id, &linux::Dirs::current()?)
    }
    #[cfg(windows)]
    {
        windows::uninstall(id, &windows::Root::current()?)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(Error::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute path on this system: Windows's have a drive.
    const PROGRAM: &str = if cfg!(windows) {
        r"C:\Program Files\App\app.exe"
    } else {
        "/usr/bin/app"
    };

    fn menu() -> Menu {
        Menu::new("packwing", "packwing")
            .item(Item::new("extract", "Extract here", PROGRAM).extensions(["ZIP", ".7z"]))
    }

    #[test]
    fn a_good_menu_passes() {
        assert!(menu().validate().is_ok());
        assert_eq!(
            menu().items[0].applies_to.files,
            Files::Extensions(vec!["zip".into(), "7z".into()])
        );
    }

    #[test]
    fn items_accept_what_they_apply_to() {
        let file = |name| Chosen {
            name,
            folder: false,
        };
        let dir = Chosen {
            name: "photos",
            folder: true,
        };
        let archives = Item::new("x", "X", PROGRAM)
            .extensions(["zip", "tar.gz"])
            .applies_to;
        assert!(archives.accepts(&[file("a.ZIP"), file("b.tar.gz")]));
        assert!(!archives.accepts(&[file("a.zip"), file("notes.txt")]));
        assert!(
            !archives.accepts(&[file("unzip")]),
            "a name, not an extension"
        );
        assert!(
            !archives.accepts(&[file(".zip")]),
            "a hidden file named zip"
        );
        assert!(!archives.accepts(&[dir]));
        assert!(!archives.accepts(&[]));

        let one_folder = Item::new("x", "X", PROGRAM)
            .folders_only()
            .count(Count::One)
            .applies_to;
        assert!(one_folder.accepts(&[dir]));
        assert!(!one_folder.accepts(&[dir, dir]));
        assert!(!one_folder.accepts(&[file("a.zip")]));

        let two = Item::new("x", "X", PROGRAM)
            .folders(true)
            .count(Count::Many)
            .applies_to;
        assert!(two.accepts(&[dir, file("a")]));
        assert!(!two.accepts(&[file("a")]));
    }

    #[test]
    fn ids_fit_file_names_and_keys() {
        for bad in [
            "",
            "Pack",
            "pack wing",
            "-pack",
            "pack-",
            "pack.wing",
            "pàck",
        ] {
            let mut m = menu();
            m.id = bad.into();
            assert!(m.validate().is_err(), "{bad:?}");
        }
        assert!(check_id("id", &"a".repeat(64)).is_ok());
        assert!(check_id("id", &"a".repeat(65)).is_err());
    }

    #[test]
    fn items_need_what_installing_needs() {
        let base = || Item::new("x", "X", PROGRAM);
        let empty = Menu::new("m", "M");
        assert!(empty.validate().is_err(), "no items");
        let twice = Menu::new("m", "M").item(base()).item(base());
        assert!(twice.validate().is_err(), "same id twice");
        for item in [
            Item::new("x", "", PROGRAM),
            Item::new("x", "Two\nlines", PROGRAM),
            Item::new("x", "X", "x"),
            base().arg("a\nb"),
            base().extensions(Vec::<String>::new()),
            base().extensions(["a b"]),
            base().extensions(["zip."]),
            base()
                .count(Count::One)
                .folders(false)
                .folders_only()
                .folders(false),
        ] {
            assert!(
                Menu::new("m", "M").item(item.clone()).validate().is_err(),
                "{item:?}"
            );
        }
        let fine = base().extensions(["tar.gz", "c++"]).folders(true);
        assert!(Menu::new("m", "M").item(fine).validate().is_ok());
    }
}
