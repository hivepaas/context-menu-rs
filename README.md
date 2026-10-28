# context-menu-rs

Put your app in the right-click menu of the system's file manager, from Rust: Windows Explorer, macOS Finder and the file managers of Linux, from one description of the menu.

> **Status:** early development. Nothing is published on crates.io yet.

## 🎯 Plans

- **One description of the menu**: its items and their icons, what each applies to (files, folders, some extensions, one or many), and the command it runs with the paths chosen.
- **Windows 11**: in the menu itself, not only under "Show more options", through `IExplorerCommand` in a COM DLL and a sparse package; the classic menu too, for Windows 10.
- **Linux**: Nautilus (GNOME Files), Dolphin, Nemo and Thunar.
- **macOS**: Finder, through Services first, then a Finder Sync extension.
- **Installing and removing** the menu for the current user, from the app itself or from its installer.

## 🐝 Made by HivePaaS

context-menu-rs is built and maintained by HivePaaS, who also make:

- [hivepaas](https://github.com/hivepaas/hivepaas): a self-hosted PaaS.
- [hivepack](https://github.com/hivepaas/hivepack-rs): read and write archives and compressed files in Rust.
- [shellwing](https://shellwing.app): a fast, native SSH client for macOS, Windows and Linux.
- [authwing](https://github.com/hivepaas/authwing): a two-factor code app for macOS, Windows and Linux that knows the window you are in.
- [packwing](https://github.com/hivepaas/packwing): a file archiver for macOS, Windows and Linux that opens archives safely, and the first app to use context-menu-rs.

## 📜 License

MIT. See [LICENSE](LICENSE). Copyright (c) 2026 HivePaaS.
