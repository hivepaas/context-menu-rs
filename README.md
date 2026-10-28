# context-menu-rs

Put your app in the right-click menu of the system's file manager, from Rust: Windows Explorer, macOS Finder and the file managers of Linux, from one description of the menu.

> **Status:** early development. Linux works; Windows comes next. Nothing is published on crates.io yet.

## ✨ Usage

```rust
use context_menu::{Count, Item, Menu};

let app = "/usr/bin/packwing";
let menu = Menu::new("packwing", "packwing")
    .submenu(true)
    .item(
        Item::new("extract-here", "Extract here", app)
            .args(["--extract-here"])
            .extensions(["zip", "7z", "rar", "tar.zst"]),
    )
    .item(
        Item::new("compress", "Compress…", app)
            .args(["--compress"])
            .folders(true),
    )
    .item(
        Item::new("compare", "Compare the two", app)
            .args(["--compare"])
            .count(Count::Many),
    );

// For the current user, when the app starts: installing again only when
// the menu, or what the system offers, changed.
if !context_menu::is_current(&menu)? {
    let report = context_menu::install(&menu)?;
    for note in &report.notes {
        println!("{note}");                   // such as "restart Nautilus"
    }
}
// Later: context_menu::uninstall("packwing")?;
```

Each item runs its program with its arguments and then the paths chosen, in one run: `/usr/bin/packwing --extract-here /home/me/a.zip /home/me/b.7z`. To try it, `cargo run --example demo -- install` puts a demo menu in your file manager, and `uninstall` takes it away.

## 🖥️ Where

| | How | State |
|---|---|---|
| **Nautilus** (GNOME) | A native extension, built once and installed by the app's package under its menu's name ([docs/nautilus.md](docs/nautilus.md)); else an extension through nautilus-python; else scripts in its Scripts submenu | ✅ |
| **Dolphin** (KDE) | Service menus | ✅ |
| **Nemo** (Cinnamon) | Actions | ✅ |
| **Thunar** (Xfce) | Custom actions | Planned |
| **Windows 11** | In the menu itself, not only under "Show more options": `IExplorerCommand` in a COM DLL and a sparse package; the classic menu too, for Windows 10 | Next |
| **macOS Finder** | Services first, then a Finder Sync extension | Planned |

What each Linux file manager can and cannot do, and where the files go: [docs/linux.md](docs/linux.md). The menu's definition, which the native extensions read: [docs/definition.md](docs/definition.md).

## 🐝 Made by HivePaaS

context-menu-rs is built and maintained by HivePaaS, who also make:

- [hivepaas](https://github.com/hivepaas/hivepaas): a self-hosted PaaS.
- [hivepack](https://github.com/hivepaas/hivepack-rs): read and write archives and compressed files in Rust.
- [shellwing](https://shellwing.app): a fast, native SSH client for macOS, Windows and Linux.
- [authwing](https://github.com/hivepaas/authwing): a two-factor code app for macOS, Windows and Linux that knows the window you are in.
- [packwing](https://github.com/hivepaas/packwing): a file archiver for macOS, Windows and Linux that opens archives safely, and the first app to use context-menu-rs.

## 📜 License

MIT. See [LICENSE](LICENSE). Copyright (c) 2026 HivePaaS.
