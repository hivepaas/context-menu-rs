//! Put your app in the file manager's Open With, for the files it opens.
//!
//! Describe it once, as a [`Handler`], and [`install`] it for the current
//! user; [`uninstall`] takes it away again. Choosing the app in Open With
//! runs its program with the arguments given and then the files chosen, as
//! an item of the menu does.
//!
//! ```no_run
//! use context_menu::open_with::{self, Handler};
//!
//! let handler = Handler::new("packwing", "packwing", "/usr/bin/packwing")
//!     .arg("--open")
//!     .extensions(["zip", "7z", "rar", "tar.zst"]);
//! // When the app starts, as for the menu.
//! if !open_with::is_current(&handler)? {
//!     open_with::install(&handler)?;
//! }
//! # Ok::<(), context_menu::Error>(())
//! ```
//!
//! The app is offered, not chosen: which app opens a type of file at a
//! double click is the user's to say, in Open With itself or in the
//! system's settings, and Windows lets no app say it for them.
//!
//! On Linux it is a desktop entry in the user's `applications` folder, which
//! every file manager reads (`docs/linux.md`); on Windows, a ProgID in the
//! user's part of the registry, listed for each extension (`docs/windows.md`).
//! On macOS an app's bundle declares the files it opens, in its
//! `Info.plist`, and there is nothing to install: [`install`] is
//! [`Error::Unsupported`] there, and `docs/macos.md` says what to declare.

use std::path::PathBuf;

use crate::{Error, Report, check_args, check_extensions, check_id, check_label, check_program};

/// An app in Open With, for the files it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handler {
    /// Names it where the system keeps it, with the same characters as
    /// [`Menu::id`](crate::Menu::id). A menu and a handler may share one.
    pub id: String,
    /// What Open With calls the app.
    pub name: String,
    /// Its icon, as an [item's](crate::Item#structfield.icon). On Windows,
    /// files the app opens by default show it too.
    pub icon: Option<String>,
    /// The files it opens: those whose names end in one of these, without
    /// the dot and in lower case: `zip`, `tar.gz`.
    pub extensions: Vec<String>,
    /// The program to run, as an absolute path.
    pub program: PathBuf,
    /// Its arguments, before the files chosen.
    pub args: Vec<String>,
}

impl Handler {
    /// A handler for no files yet, running `program` with no arguments
    /// before the files.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        program: impl Into<PathBuf>,
    ) -> Self {
        Handler {
            id: id.into(),
            name: name.into(),
            icon: None,
            extensions: Vec::new(),
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// The app's icon: see [`icon`](Self#structfield.icon).
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Adds an argument, before the files.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Adds arguments, before the files.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Adds the files with these extensions: `zip` or `.zip`, any case.
    pub fn extensions(mut self, extensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        for ext in crate::extensions(extensions) {
            if !self.extensions.contains(&ext) {
                self.extensions.push(ext);
            }
        }
        self
    }

    /// Checks everything [`install`] needs: the id, the name, the program
    /// and the extensions.
    pub fn validate(&self) -> Result<(), Error> {
        check_id("the handler's id", &self.id)?;
        check_label("the handler's name", &self.name)?;
        let of = format!("the handler {:?}", self.id);
        check_program(&of, &self.program)?;
        check_args(&of, &self.args)?;
        check_extensions(&of, &self.extensions)
    }
}

/// Puts `handler` in Open With for the current user, in place of an
/// earlier install of the same handler.
pub fn install(handler: &Handler) -> Result<Report, Error> {
    handler.validate()?;
    #[cfg(target_os = "linux")]
    {
        crate::linux::open_with::install(handler, &crate::linux::Dirs::current()?)
    }
    #[cfg(windows)]
    {
        crate::windows::open_with::install(handler, &crate::windows::Root::current()?)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(Error::Unsupported)
    }
}

/// Whether `handler` is installed for the current user as [`install`] would
/// install it now: false when it is not, when it changed, or when what the
/// system knows of its extensions did. Cheap enough to call when the app
/// starts, and [`install`] when false.
pub fn is_current(handler: &Handler) -> Result<bool, Error> {
    handler.validate()?;
    #[cfg(target_os = "linux")]
    {
        crate::linux::open_with::is_current(handler, &crate::linux::Dirs::current()?)
    }
    #[cfg(windows)]
    {
        crate::windows::open_with::is_current(handler, &crate::windows::Root::current()?)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(Error::Unsupported)
    }
}

/// Takes the handler `id` out of Open With for the current user. Nothing
/// installed is not an error.
pub fn uninstall(id: &str) -> Result<Report, Error> {
    check_id("the handler's id", id)?;
    #[cfg(target_os = "linux")]
    {
        crate::linux::open_with::uninstall(id, &crate::linux::Dirs::current()?)
    }
    #[cfg(windows)]
    {
        crate::windows::open_with::uninstall(id, &crate::windows::Root::current()?)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Err(Error::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM: &str = if cfg!(windows) {
        r"C:\Program Files\App\app.exe"
    } else {
        "/usr/bin/app"
    };

    #[test]
    fn a_handler_needs_what_installing_needs() {
        let base = || Handler::new("packwing", "packwing", PROGRAM).extensions(["ZIP", ".7z"]);
        assert!(base().validate().is_ok());
        assert_eq!(
            base().extensions(["zip", "rar"]).extensions,
            ["zip", "7z", "rar"]
        );
        for bad in [
            Handler::new("packwing", "packwing", PROGRAM),
            Handler::new("Pack", "packwing", PROGRAM).extensions(["zip"]),
            Handler::new("packwing", "", PROGRAM).extensions(["zip"]),
            Handler::new("packwing", "two\nlines", PROGRAM).extensions(["zip"]),
            Handler::new("packwing", "packwing", "app").extensions(["zip"]),
            base().arg("a\nb"),
            base().extensions(["a b"]),
            base().extensions(["zip."]),
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        let Err(Error::Invalid(why)) = Handler::new("packwing", "packwing", PROGRAM).validate()
        else {
            panic!("no extensions is invalid");
        };
        assert_eq!(why, "the handler \"packwing\" has no extensions");
    }
}
