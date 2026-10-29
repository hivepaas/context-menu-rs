# Windows

Windows has two right-click menus. The classic one, built from verbs in the registry, is the menu itself on Windows 10, and on Windows 11 the one under "Show more options" (Shift+F10, or Shift and a right click). Windows 11's own menu takes items only from packaged apps: a COM class implementing `IExplorerCommand`, declared in a package's manifest.

So there are two ways in:

- **`install` alone** writes the classic menu, as verbs in the user's registry, for any app, with no package. Windows 11 shows it under "Show more options", and it has the limits below.
- **With the native extension**, `context-menu-windows`, which an app's package declares, the menu is in Windows 11's own menu and in the classic one, showing exactly the items for what is chosen, as on Linux. `install` sees the package and leaves the registry to it.

## The native extension

It is one library, `context-menu-windows`, built once for everybody's menus, like Nautilus's ([nautilus.md](nautilus.md)). An app ships a copy named after its menu in its own folder, `context-menu-<menu id>.dll` (`context_menu::windows::native_name`), and its package declares the copy as a COM class made from the menu's id (`context_menu::windows::clsid`):

```text
C:\Program Files\Photo Kit\photokit.exe
C:\Program Files\Photo Kit\context-menu-photo-kit.dll
```

When Explorer loads it, the library finds its own name, and so its menu. Each time Explorer asks what to show, it reads the menu's definition, which `install` writes in `%LOCALAPPDATA%\context-menu-rs\<menu>.menu` ([definition.md](definition.md)). So:

- The app installs its menu as on any other system, with `install`, for each user. Changing the menu (the app's settings turning an item off, say) is `install` again; Explorer shows the change at the next right click.
- With the package registered for the user, `install` writes no verbs, and removes those an earlier install wrote: Windows shows the package's command under "Show more options" too, so they would show twice. `is_current` is false when a package comes or goes. Windows says which packages declare the menu's class in `HKEY_LOCAL_MACHINE\SOFTWARE\Classes\PackagedCom\ClassIndex`, and which packages are the user's in `HKEY_CURRENT_USER\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages`.
- A copy whose menu is not installed for a user shows nothing for that user.

What the menu shows:

- **One entry**, as Windows 11's menu takes one per app: a submenu with the menu's title and its items, or, for a menu of one item and no submenu, that item. Windows asks whether the entry has a submenu before anything is chosen, so it cannot depend on what is.
- **Each item for what it applies to**, all of it, as on Linux (`AppliesTo::accepts`): `Count::One` and `Count::Many` count what is chosen, `tar.gz` is told from `gz`, and with a zip and a folder chosen, an item for archives does not show. The entry shows when any of its items does.
- **One run with every path**: the program, its arguments, and the paths chosen, started in the first one's folder.
- **Icons**, the menu's and the items', when they are an `.ico`, or an `.exe` or `.dll` with an index.

### For packagers

Get the library from the [releases](https://github.com/hivepaas/context-menu-rs/releases), `context-menu-windows-x86_64.dll` or `-aarch64.dll`, or build it:

```powershell
$env:RUSTFLAGS = '-C target-feature=+crt-static'   # no Visual C++ runtime to install
cargo build --release -p context-menu-windows       # target\release\context_menu_windows.dll
```

Ship it in the app's folder as `context-menu-<menu id>.dll`, the id you give `Menu::new`.

The package is a *package with external location* (Microsoft's "Grant package identity by packaging with external location"): a manifest, signed, whose files are the app's folder as installed. It gives the app an identity and declares the extension, with no other change to the app or its installer. Its manifest has:

- the app's `Identity`, whose `Publisher` is the signing certificate's subject;
- `<uap10:AllowExternalContent>true</uap10:AllowExternalContent>` in its `Properties`;
- an `Application` with `uap10:TrustLevel="mediumIL"` and `uap10:RuntimeBehavior="win32App"`;
- the capabilities `runFullTrust` and `unvirtualizedResources`;
- and in the `Application`'s `Extensions`, what `context_menu::windows::package_extensions(id)` returns: the menu for any file and any folder, served by the copy.

`cargo run --example demo -- manifest` prints a whole manifest, the demo's, to start from. Its paths, the program's, the logos' and the extension's, are in the app's folder. Then, with the Windows SDK:

```powershell
makeappx pack /d package-folder /p photokit.msix /nv   # package-folder holds AppxManifest.xml
signtool sign /fd SHA256 /f certificate.pfx photokit.msix
```

The app's installer registers the package for the user once the app's files are in place, with no rights beyond the user's: `Add-AppxPackage -Path photokit.msix -ExternalLocation "C:\Program Files\Photo Kit"`, or `PackageManager.AddPackageByUriAsync` with `ExternalLocationUri`. Windows checks the signature, so the certificate must be one the machine trusts, as a code-signing certificate from a certificate authority is. The app's next `is_current` is false, and `install` moves the menu to the package. The uninstaller removes the package (`Remove-AppxPackage`) and calls `uninstall`.

Windows 10 registers these packages from version 2004; whether its menu shows the command has not been tried.

### How it is made

`src/lib.rs` decides, in plain Rust tested on every system: which menu the library's name gives, what shows for the files chosen, and what runs. `src/ffi.rs` is the COM side, and the only unsafe code: `DllGetClassObject`, which hands out a class factory for the menu's class alone, and `DllCanUnloadNow`; the `IExplorerCommand`s of the menu and its items; the `IEnumExplorerCommand` listing the items. It reads the files chosen from the `IShellItemArray` Explorer gives. Every function Explorer calls catches a panic and fails instead.

`tests/explorer.rs` loads the library as Explorer does, renamed for a menu: it gets the class through `DllGetClassObject`, asks the commands what they show for files and folders made for the test, given as an `IShellItemArray` from the shell, and chooses an item, whose program writes down the paths it gets.

## The classic menu

Without a package serving the menu, `install` writes verbs in the user's part of the registry, `HKEY_CURRENT_USER\Software\Classes`, so it needs no rights beyond the user's, and Explorer shows them at once. An item is a verb under each kind of thing it applies to:

| The item applies to | Its verb goes under |
|---|---|
| Any file | `*\shell` |
| Files with an extension | `SystemFileAssociations\.<extension>\shell` |
| Folders | `Directory\shell` |

With the items grouped in a submenu, each kind gets a verb named after the menu, with `MUIVerb` (the title) and `SubCommands` empty, and the items it applies to as verbs in its own `shell` key. Explorer shows one verb of a name, taking an extension's over `*`'s, so the submenu under an extension holds the items for any file as well. Without a submenu, each item is a verb named `<menu>.<n>.<item>`. Windows orders verbs by their keys' names, so the items' keys start with their place in the menu (`00.extract`). Every verb of the menu's carries a value `ContextMenuRs` naming the menu: `uninstall` removes the keys that carry it, and nothing else. `install` writes the menu's definition too, for the native extension.

What the classic menu can and cannot do:

- **Windows runs the program once for each file chosen**, with that file's path. An item for several files at once (`Count::Many`) cannot get them in one run; an app that wants them together has to gather the runs itself, as a single instance. The others show for any number (`MultiSelectModel` `Player`, past Windows's limit of 15).
- **`Count::One` hides an item when more than one is chosen** (`MultiSelectModel` `Single`), but in a submenu only when all of a kind's items are `Count::One`: Windows reads the value for the submenu's own verb, not for those in it. Otherwise the item shows, and runs once for each.
- **With different things chosen**, files and folders or a zip and a text file, the menu is that of the one right-clicked, and the item chosen runs for every one of them, even those it is not for. An app has to check the paths it gets.
- **Windows knows a file's last extension only.** An item for `tar.gz` is a verb under `.gz` with an `AppliesTo` condition naming the longer extension (`System.FileName:~>".tar.gz"`), and so is a submenu when all its items have one, or it would show empty for other `.gz` files. Extensions match in any case.
- **Icons** are an `.ico`, or an `.exe` or `.dll` with an index (`C:\Program Files\App\app.exe,0`). An icon theme's name, for Linux, is left out.
- **The command** is the program and its arguments, each quoted as `CommandLineToArgvW` reads it back, and then `"%1"`. Windows replaces its own codes in it (`%1`, `%V`, `%L`, `%W`…), so an argument holding a `%` and a digit or one of those letters may change.

## Trying it

`cargo run --example demo -- install` puts the demo menu in the classic menu, for files, archives and folders; choosing an item shows the paths in a message box, and writes them to `%TEMP%\context-menu-demo.log`. `uninstall` takes it away.

For Windows 11's own menu, turn on Developer Mode (Settings, System, For developers), which registers packages that are not signed, and then, in a PowerShell on the desktop (Windows refuses it over SSH):

```powershell
cargo build --example demo
cargo build -p context-menu-windows
.\scripts\windows\demo-package.ps1           # -Remove takes it away
```

It registers a package for the demo, from the manifest `demo manifest` writes, with the extension in the demo's folder, and installs the demo's menu again, which moves it to the package.

`scripts/windows/menu.ps1` prints the classic menu as the shell builds it from the registry, submenus and all, and can choose an item, with no window: `.\menu.ps1 C:\Users\me\a.zip`, or `-Choose 'context-menu demo/Archives: show their paths'`. A package's command is not in it: Explorer adds that itself.

`tests/windows.rs` writes the menu to a scratch key (`HKEY_CURRENT_USER\Software\context-menu-rs-test-…`), not the one Explorer reads, and reads it back: what is written, `is_current` before and after a change, a key changed by hand, that other apps' verbs stay, and the classic menu leaving and coming back as a package serving the menu is registered and removed, with Windows's lists of packages kept under the scratch key too.
