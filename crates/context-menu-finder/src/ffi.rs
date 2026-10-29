//! Finder's and the system's Objective-C interfaces, through objc2: the
//! extension's principal class, a `FIFinderSync`, handed to Finder by
//! `NSExtensionMain`; and the helper, which needs no more than Foundation.
//!
//! Every method Finder calls catches a panic and shows nothing instead, so
//! none crosses into Finder.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, OsStr, c_char, c_int};
use std::os::unix::ffi::OsStrExt as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::ptr::NonNull;
use std::time::SystemTime;

use block2::RcBlock;
use context_menu::macos::{GROUP_KEY, HELPER_PATH, MENU_KEY};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSMenu, NSMenuItem, NSWorkspace, NSWorkspaceDidMountNotification,
    NSWorkspaceDidUnmountNotification, NSWorkspaceOpenConfiguration,
};
use objc2_finder_sync::{FIFinderSync, FIFinderSyncController, FIFinderSyncProtocol, FIMenuKind};
use objc2_foundation::{
    NSBundle, NSFileManager, NSNotification, NSOperationQueue, NSSet, NSString, NSURL,
    NSVolumeEnumerationOptions, ns_string,
};

use super::{Picked, Request, command, leave, load, requests, shown, take};

unsafe extern "C" {
    /// Foundation's entry point for an app extension: it makes the
    /// `NSExtensionPrincipalClass` and serves Finder until it is done.
    fn NSExtensionMain(argc: c_int, argv: *mut *mut c_char) -> c_int;
}

/// Runs the program as its bundle makes it: an extension's `Info.plist`
/// has `NSExtension`; the helper's has not.
pub(crate) fn run() -> ExitCode {
    if info("NSExtension").is_some() {
        extension()
    } else {
        helper()
    }
}

/// A value of the bundle's `Info.plist`, as text, or as nothing when it is
/// not text.
fn info(key: &str) -> Option<String> {
    let value = NSBundle::mainBundle().objectForInfoDictionaryKey(&NSString::from_str(key))?;
    Some(match value.downcast::<NSString>() {
        Ok(text) => text.to_string(),
        Err(_) => String::new(),
    })
}

/// The user's home, from the system's list of users: in the sandbox, `HOME`
/// is the extension's own container.
fn home() -> Option<PathBuf> {
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0 as c_char; 4096];
    // SAFETY: getpwuid_r writes the entry and the strings it points to in
    // `entry` and `buffer`, both ours and as large as said, and sets
    // `result` to `entry` on success.
    let found = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            entry.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    if found != 0 || result.is_null() {
        return None;
    }
    // SAFETY: on success the entry is written, and its pw_dir is a C string
    // in `buffer`, still alive.
    let dir = unsafe { (*result).pw_dir };
    if dir.is_null() {
        return None;
    }
    let dir = unsafe { CStr::from_ptr(dir) };
    Some(PathBuf::from(OsStr::from_bytes(dir.to_bytes()))).filter(|p| p.is_absolute())
}

/// The app group's folder, which the extension and the helper share.
fn group() -> Option<PathBuf> {
    NSFileManager::defaultManager()
        .containerURLForSecurityApplicationGroupIdentifier(&NSString::from_str(&info(GROUP_KEY)?))?
        .to_file_path()
}

/// Whether the folder at `path` is one Finder shows as a file, as the
/// system says.
pub(crate) fn is_package(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|p| NSWorkspace::sharedWorkspace().isFilePackageAtPath(&NSString::from_str(p)))
}

/// Runs `f`, or nothing when it panics: no panic crosses into Finder.
fn guard<R>(f: impl FnOnce() -> Option<R>) -> Option<R> {
    catch_unwind(AssertUnwindSafe(f)).ok().flatten()
}

// The helper.

/// Starts the programs of the requests waiting, for the menu it serves,
/// then ends: it was opened for them.
fn helper() -> ExitCode {
    let started = (|| {
        let menu = load(&info(MENU_KEY)?, &home()?).ok()?;
        let mut started = 0;
        for request in take(&requests(&group()?), SystemTime::now()) {
            // The program goes on alone: it has a process group of its own,
            // and is the system's to reap once the helper has ended.
            if let Some(Ok(_)) = command(&menu, &request).map(|mut c| c.spawn()) {
                started += 1;
            }
        }
        Some(started)
    })();
    match started {
        Some(_) => ExitCode::SUCCESS,
        None => ExitCode::FAILURE,
    }
}

// The extension.

/// Hands the principal class to `NSExtensionMain`, which serves Finder.
fn extension() -> ExitCode {
    // Registered now, so that NSExtensionMain finds it by its name.
    let _ = Extension::class();
    let args: Vec<CString> = std::env::args_os()
        .filter_map(|a| CString::new(a.as_bytes()).ok())
        .collect();
    let mut argv: Vec<*mut c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    let argc = c_int::try_from(args.len()).unwrap_or(0);
    // SAFETY: argv holds argc C strings, alive until the end of main, and a
    // null after them, as main's own.
    let code = unsafe { NSExtensionMain(argc, argv.as_mut_ptr()) };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// Shows the menu in every folder: the startup disk's, under `/`, and each
/// other volume's, which `/` does not take in.
fn watch() {
    let mut urls = vec![NSURL::fileURLWithPath(ns_string!("/"))];
    if let Some(volumes) = NSFileManager::defaultManager()
        .mountedVolumeURLsIncludingResourceValuesForKeys_options(
            None,
            NSVolumeEnumerationOptions::SkipHiddenVolumes,
        )
    {
        urls.extend(volumes.iter());
    }
    let set = NSSet::from_retained_slice(&urls);
    // SAFETY: the controller is the extension's, which is running.
    unsafe { FIFinderSyncController::defaultController().setDirectoryURLs(Some(&set)) };
}

/// The files and folders chosen in Finder now.
fn picked() -> Vec<Picked> {
    // SAFETY: as in watch.
    let urls = unsafe { FIFinderSyncController::defaultController().selectedItemURLs() };
    urls.map(|urls| {
        urls.iter()
            .filter_map(|url| {
                let path = url.to_file_path()?;
                let folder = path.is_dir() && !is_package(&path);
                Picked::at(path, folder)
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The menu for the files chosen: the items that apply to them, in a
/// submenu when the menu asks for one; none when no item does.
fn menu(kind: FIMenuKind) -> Option<Retained<NSMenu>> {
    if kind != FIMenuKind::ContextualMenuForItems {
        return None;
    }
    let menu = load(&info(MENU_KEY)?, &home()?).ok()?;
    let shown = shown(&menu, &picked());
    if shown.is_empty() {
        return None;
    }
    // SAFETY: Finder asks for the menu on one of the extension's own
    // threads, not the main one, and takes it there: it is built, never
    // shown, in this process, as Apple's own Finder Sync template builds it.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let root = NSMenu::new(mtm);
    let title = NSString::from_str(&menu.title);
    let target = if menu.submenu {
        NSMenu::initWithTitle(NSMenu::alloc(mtm), &title)
    } else {
        root.clone()
    };
    for (place, item) in shown {
        // SAFETY: `choose:` is the extension's own method, taking the item.
        let entry = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(&item.label),
                Some(sel!(choose:)),
                ns_string!(""),
            )
        };
        entry.setTag(isize::try_from(place).ok()?);
        target.addItem(&entry);
    }
    if menu.submenu {
        // SAFETY: an item with no action, holding the submenu.
        let parent = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &title,
                None,
                ns_string!(""),
            )
        };
        parent.setSubmenu(Some(&target));
        root.addItem(&parent);
    }
    Some(root)
}

/// The item at `place` in the menu chosen, for the files chosen: a request
/// left for the helper, and the helper opened. Finder keeps an item's tag,
/// its place, but not the rest of what the extension gave it.
fn choose(place: isize) -> Option<()> {
    let menu = load(&info(MENU_KEY)?, &home()?).ok()?;
    let item = menu.items.get(usize::try_from(place).ok()?)?;
    let request = Request {
        item: item.id.clone(),
        paths: picked().into_iter().map(|p| p.path).collect(),
    };
    leave(&requests(&group()?), &request).ok()?;
    // The helper is in the app's bundle, which holds the extension in
    // Contents/PlugIns.
    let extension = PathBuf::from(NSBundle::mainBundle().bundlePath().to_string());
    let app = extension.parent()?.parent()?.parent()?;
    let helper = NSURL::fileURLWithPath(&NSString::from_str(app.join(HELPER_PATH).to_str()?));
    let config = NSWorkspaceOpenConfiguration::configuration();
    config.setActivates(false);
    // A helper of its own each time, which takes this request even when
    // another is ending.
    config.setCreatesNewApplicationInstance(true);
    NSWorkspace::sharedWorkspace()
        .openApplicationAtURL_configuration_completionHandler(&helper, &config, None);
    Some(())
}

/// What the extension keeps: its observers of volumes coming and going.
struct Ivars {
    _observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

define_class!(
    // SAFETY: FIFinderSync is made to be subclassed, and Extension does not
    // implement Drop.
    #[unsafe(super(FIFinderSync, NSObject))]
    #[name = "ContextMenuRsFinderSync"]
    #[ivars = Ivars]
    struct Extension;

    impl Extension {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Option<Retained<Self>> {
            let observers = guard(|| Some(observe())).unwrap_or_default();
            let this = this.set_ivars(Ivars {
                _observers: observers,
            });
            // SAFETY: NSObject's init, which FIFinderSync's is.
            let this: Option<Retained<Self>> = unsafe { msg_send![super(this), init] };
            guard(|| {
                watch();
                Some(())
            });
            this
        }

        /// An item chosen, as `sender`, a menu item: Finder calls it on one
        /// of the extension's own threads, as it asks for the menu.
        #[unsafe(method(choose:))]
        fn choose(&self, sender: &NSMenuItem) {
            let place = sender.tag();
            guard(|| choose(place));
        }
    }

    unsafe impl NSObjectProtocol for Extension {}

    unsafe impl FIFinderSyncProtocol for Extension {
        #[unsafe(method_id(menuForMenuKind:))]
        fn menu_for_menu_kind(&self, kind: FIMenuKind) -> Option<Retained<NSMenu>> {
            guard(|| menu(kind))
        }
    }
);

/// Watches again whenever a volume comes or goes, on the main queue.
fn observe() -> Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>> {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let queue = NSOperationQueue::mainQueue();
    let block = RcBlock::new(|_: NonNull<NSNotification>| {
        guard(|| {
            watch();
            Some(())
        });
    });
    // SAFETY: the names are the system's statics; the block holds nothing
    // and may run on the main queue at any time.
    unsafe {
        [
            NSWorkspaceDidMountNotification,
            NSWorkspaceDidUnmountNotification,
        ]
        .into_iter()
        .map(|name| {
            center.addObserverForName_object_queue_usingBlock(
                Some(name),
                None,
                Some(&queue),
                &block,
            )
        })
        .collect()
    }
}
