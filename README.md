# context-menu-rs

Put your app in the right-click menu of the system's file manager, from Rust: Windows Explorer, macOS Finder and the file managers of Linux, from one description of the menu. And in their Open With, for the files your app opens.

> **Status:** early development: 0.2.0 adds Open With. Linux, Windows and macOS work.

## ✨ Usage

```sh
cargo add context-menu
```

### Describe the menu

A `Menu` holds `Item`s. Each item runs a program you name, with your arguments and then the paths of the files and folders chosen, all in one run.

```rust
use context_menu::{Count, Item, Menu};

let app = "/usr/bin/packwing";                // an absolute path
let menu = Menu::new("packwing", "Pack Wing")   // an id, and the submenu's title
    .submenu(true)                            // the items under one entry
    .item(
        Item::new("extract-here", "Extract here", app)
            .args(["--extract-here"])
            .extensions(["zip", "7z", "rar", "tar.zst"]),
    )
    .item(
        Item::new("compress", "Compress…", app)
            .args(["--compress"])
            .folders(true),                   // files and folders
    )
    .item(
        Item::new("compare", "Compare the two", app)
            .args(["--compare"])
            .count(Count::Many),              // two or more chosen
    );
```

| | What it says |
|---|---|
| `Menu::new(id, title)` | The id names the menu where the system keeps it: 1 to 64 lower-case letters, digits and inner `-`. The title labels the submenu. |
| `.submenu(true)` | Groups the items under one entry, labelled with the title. |
| `Item::new(id, label, program)` | An item for any one or more files, not folders, running `program`, an absolute path. Its id is unique in the menu, with the same characters as the menu's. |
| `.arg(…)`, `.args([…])` | Arguments, before the paths. |
| `.extensions([…])` | Only files with these extensions, `zip` or `.zip`, in any case; longer ones too, such as `tar.gz`. |
| `.folders(true)`, `.folders_only()` | Folders as well as the files, or folders alone. |
| `.count(…)` | How many may be chosen: `Count::One`, `Count::Many` (two or more) or `Count::Any` (one or more, the default). |
| `.icon(…)` | The menu's or an item's icon: see below. |

With several files and folders chosen, an item shows when it applies to every one of them: with a zip and a text file chosen, "Extract here" does not show.

### Install it, and keep it current

`install` puts the menu in the file managers for the current user, with no administrator rights. `is_current` says whether that is still done, and is cheap, so call it when your app starts:

```rust
if !context_menu::is_current(&menu)? {
    let report = context_menu::install(&menu)?;
    for note in &report.notes {
        println!("{note}");      // what the user may need to know, such as "restart Nautilus"
    }
}
```

`is_current` is false when the menu changed, or when what the system offers did: a native extension or nautilus-python installed or removed, or on macOS an app declaring a file type. Call `install` again whenever the menu changes, after the user turns an item off in your settings, say: it replaces the earlier install, and leaves nothing of it behind.

`context_menu::uninstall("packwing")` takes the menu away, when the user turns it off, or from your uninstaller: nothing else does it for you. The `Report` that `install` and `uninstall` return lists the files and registry keys written and removed.

### What your program gets

An item chosen runs its program once, with its arguments and then every path chosen:

```text
/usr/bin/packwing --extract-here /home/me/a.zip /home/me/b.7z
```

So your program reads them as any command line:

```rust
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((what, paths)) if what == "--extract-here" => extract_here(paths),
        Some((what, paths)) if what == "--compress" => compress(paths),
        _ => run_the_app(),
    }
}
```

Check what you get: where a system cannot do all a menu asks, an item may show, or run, for more than it applies to. Windows's classic menu runs the program once for each file, and macOS's Quick Actions cannot count what is chosen; the report's notes say when, and each system's page tells it all.

### Icons

An icon is a string, which means something on each system, so give each its own:

| System | An icon is |
|---|---|
| Linux | An icon theme's name, such as `package-x-generic`, or an absolute path to an image |
| Windows | An `.ico` file, or an `.exe` or `.dll` and an index: `C:\Program Files\Pack Wing\packwing.exe,0` |
| macOS | An SF Symbol's name, such as `archivebox`; one of AppKit's images, such as `NSFolder`; or an absolute path to an image |

```rust
let icon = if cfg!(windows) {
    r"C:\Program Files\Pack Wing\packwing.exe,0"
} else if cfg!(target_os = "macos") {
    "archivebox"
} else {
    "package-x-generic"
};
let item = Item::new("extract-here", "Extract here", app).icon(icon);
```

Not every file manager shows them all: Nautilus shows none, Nemo no paths, and macOS's Quick Actions no SF Symbols.

### The native extensions

`install` alone works for any app. For the best menu, ship a native extension with your app: then the menu is in the file manager's own menu, grouped, and each item shows only for what it applies to. Each is built once for every app, and is in the [releases](https://github.com/hivepaas/context-menu-rs/releases):

| For | Ship | How |
|---|---|---|
| Nautilus | `libcontext-menu-nautilus-*.so`, in Nautilus's extension folder, named after your menu | [docs/nautilus.md](docs/nautilus.md) |
| Windows 11's own menu | `context-menu-windows-*.dll`, in your app's folder, named after your menu and declared by its package | [docs/windows.md](docs/windows.md) |
| Finder's own menu | `context-menu-finder-macos`, twice in your app's bundle, with the `Info.plist`s and entitlements `context_menu::macos::FinderExtension` writes | [docs/macos.md](docs/macos.md) |

Your app goes on calling `install` and `is_current` as above: they find the extension, and use it.

### Open With

An app that opens some files can put itself in the file manager's Open With for them, with a `Handler`:

```rust
use context_menu::open_with::{self, Handler};

let handler = Handler::new("packwing", "Pack Wing", app)   // an id, and the name Open With shows
    .arg("--open")
    .extensions(["zip", "7z", "rar", "tar.zst"]);
if !open_with::is_current(&handler)? {
    open_with::install(&handler)?;
}
```

Choosing the app there runs its program as an item does, with its arguments and the files chosen: `packwing --open /home/me/a.zip`. It is offered, not chosen: which app opens those files at a double click stays the user's to say, and Windows lets no app say it for them. `open_with::uninstall("packwing")` takes it away.

| System | How |
|---|---|
| Linux | A desktop entry in the user's `~/.local/share/applications`, hidden from the app menus, and listed in GLib's cache there: Open With in every file manager ([docs/linux.md](docs/linux.md#open-with)). A package's own desktop entry can do it instead. |
| Windows | A ProgID in the user's registry, listed for each extension: Open With, and Settings' Default apps, whose page for the app `windows::open_with::settings_uri` gives, to send the user to ([docs/windows.md](docs/windows.md#open-with)) |
| macOS | Nothing to install: the app's `Info.plist` declares the files it opens, and they reach it as an event, not as arguments ([docs/macos.md](docs/macos.md#open-with)). `install` is `Error::Unsupported` there. |

### Try it

`cargo run --example demo -- install` puts a demo menu in your file manager, whose items show the paths they get, and on Linux and Windows the demo in Open With for archives; `uninstall` takes it away, and `status` says whether `install` would change anything. For the native extensions, `scripts/windows/demo-package.ps1` and `scripts/macos/demo-app.sh` set the demo up with them.

## 🖥️ Where

| | How | State |
|---|---|---|
| **Nautilus** (GNOME) | A native extension, built once and installed by the app's package under its menu's name ([docs/nautilus.md](docs/nautilus.md)); else an extension through nautilus-python; else scripts in its Scripts submenu | ✅ |
| **Dolphin** (KDE) | Service menus | ✅ |
| **Nemo** (Cinnamon) | Actions | ✅ |
| **Thunar** (Xfce) | Custom actions, merged with the user's own | ✅ |
| **Windows**, the classic menu | Verbs in the user's registry: the menu on Windows 10, and on Windows 11 under "Show more options" ([docs/windows.md](docs/windows.md)) | ✅ |
| **Windows 11**, its own menu | A native extension (`IExplorerCommand`) that an app's package declares, built once and named after each menu, as Nautilus's; it serves the classic menu too | ✅ |
| **macOS Finder**, its own menu | A native Finder Sync extension (`context-menu-finder`) that an app's bundle holds, built once, as Nautilus's and Windows 11's are ([docs/macos.md](docs/macos.md)) | ✅ |
| **macOS Finder**, Quick Actions | Without the extension: a workflow per item in the user's `~/Library/Services`, in Finder's Quick Actions submenu, for the file types each item applies to | ✅ |

What each Linux file manager can and cannot do, and where the files go: [docs/linux.md](docs/linux.md). The menu's definition, which the native extensions read: [docs/definition.md](docs/definition.md).

## 🐝 Made by HivePaaS

context-menu-rs is built and maintained by HivePaaS, who also make:

- [hivepaas](https://github.com/hivepaas/hivepaas): a self-hosted PaaS.
- [hivepack](https://github.com/hivepaas/hivepack-rs): read and write archives and compressed files in Rust.
- [shellwing](https://shellwing.app): a fast, native SSH client for macOS, Windows and Linux.
- [authwing](https://github.com/hivepaas/authwing): a two-factor code app for macOS, Windows and Linux that knows the window you are in.
- [packwing](https://github.com/hivepaas/packwing): a file archiver for macOS, Windows and Linux that opens archives safely, and the first app to use context-menu-rs.

⭐ If context-menu-rs, or any of these, is useful to you, please give it a star on GitHub: it helps others find it, and helps us keep making them.

## 📜 License

MIT. See [LICENSE](LICENSE). Copyright (c) 2026 HivePaaS.
