# The native Nautilus extension

Nautilus takes menu items only from extensions: libraries it loads from one folder of the system's, fixed when it was built. A per-user install cannot put one there, which is why, without one, the menu is a Python extension (with nautilus-python installed) or scripts in Nautilus's Scripts submenu (without it). The native extension needs neither: the items show in the menu itself, for the files they apply to.

It is one library, `context-menu-nautilus`, built once for everybody's menus. An app's package installs a copy named after its menu, and the copy serves that menu alone:

```text
/usr/lib/x86_64-linux-gnu/nautilus/extensions-4/libcontext-menu-photokit.so
```

When Nautilus loads it, the library finds its own name, `libcontext-menu-<menu id>.so`, and so its menu. Each time Nautilus asks what to show, it reads that menu's definition, which `context_menu::install` writes in the user's data folder (`~/.local/share/context-menu-rs/photokit.menu`; its format is in [definition.md](definition.md)). So:

- The app installs its menu as on any other system, with `install`, for each user, with no root. Changing the menu (the app's settings turning an item off, say) is `install` again; Nautilus shows the change at the next click.
- `install` sees the copy in Nautilus's folder and writes no scripts and no Python extension for Nautilus; Dolphin and Nemo get their files as always. Without the copy, it falls back on Python or scripts. `context_menu::is_current` says when to install again, such as after the package was installed or removed.
- Several apps' copies sit side by side. Nautilus loads each on its own, and each registers a type named after its menu.
- A copy whose menu is not installed for a user shows nothing for that user.

## For packagers

Get the library from the [releases](https://github.com/hivepaas/context-menu-rs/releases), `libcontext-menu-nautilus-x86_64.so` or `-aarch64.so`, or build it:

```sh
cargo build --release -p context-menu-nautilus   # target/release/libcontext_menu_nautilus.so
```

It needs Nautilus 43 or later (its extension API 4), and glibc 2.35 or later as released. Building it needs no Nautilus or GLib development files: the symbols it uses are Nautilus's own, found when Nautilus loads it.

Install it in Nautilus's extension folder under your menu's id, the one you give `Menu::new`. `pkg-config --variable=extensiondir libnautilus-extension-4` says where the folder is, with Nautilus's development files installed; otherwise it is `$libdir/nautilus/extensions-4`.

```sh
# Debian (debian/rules, or the install step of any build)
install -Dm644 libcontext-menu-nautilus-x86_64.so \
  "$DESTDIR/usr/lib/x86_64-linux-gnu/nautilus/extensions-4/libcontext-menu-photokit.so"

# Fedora (%install in the spec)
install -Dm644 libcontext-menu-nautilus-x86_64.so \
  %{buildroot}%{_libdir}/nautilus/extensions-4/libcontext-menu-photokit.so

# Arch (package() in the PKGBUILD)
install -Dm644 libcontext-menu-nautilus-x86_64.so \
  "$pkgdir/usr/lib/nautilus/extensions-4/libcontext-menu-photokit.so"
```

Nautilus loads extensions when it starts, so after the package is installed the menu shows from the next Nautilus (`nautilus -q` quits the running one). Make the package depend on Nautilus only if the app needs it; without Nautilus the library is never loaded.

Not possible: a Flatpak or Snap cannot put files in the system's folders, and an AppImage only with root. Those apps get the Python extension or the scripts, through `install`.

## How it is made

`src/lib.rs` decides, in plain Rust: which menu the library's name gives, what the menu shows for the files chosen (`AppliesTo::accepts`, as on every system), and what runs. `src/ffi.rs` is Nautilus's C interface, and the only unsafe code: the three functions an extension exports, a GObject type implementing `NautilusMenuProvider`, and the menu items, following libnautilus-extension-4's headers. Every function Nautilus calls catches a panic and shows nothing instead. A chosen item's program starts without Nautilus waiting for it, and a thread reaps it when it ends.

`tests/nautilus.rs` loads the library as Nautilus does, with Nautilus's own `libnautilus-extension.so.4`: a C harness (`tests/harness.c`) registers it with a `GTypeModule`, asks it what it shows for choices of files, made of stand-ins for Nautilus's files, and chooses an item. GLib criticals abort it, so a reference dropped twice fails the test. It needs a C compiler and GLib's headers, and is skipped without them unless `CONTEXT_MENU_NAUTILUS_REQUIRED` is set, as CI sets it.
