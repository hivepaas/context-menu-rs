# Windows

Windows has two right-click menus. The classic one, built from verbs in the registry, is the menu itself on Windows 10, and on Windows 11 the one under "Show more options" (Shift+F10, or Shift and a right click). Windows 11's own menu takes items only from packaged apps: a COM class implementing `IExplorerCommand`, declared in a package's manifest. `install` writes the classic menu now; Windows 11's own is next, as a native extension that an app's package declares, like Nautilus's (see [nautilus.md](nautilus.md)).

## The classic menu

`install` writes verbs in the user's part of the registry, `HKEY_CURRENT_USER\Software\Classes`, so it needs no rights beyond the user's, and Explorer shows them at once. An item is a verb under each kind of thing it applies to:

| The item applies to | Its verb goes under |
|---|---|
| Any file | `*\shell` |
| Files with an extension | `SystemFileAssociations\.<extension>\shell` |
| Folders | `Directory\shell` |

With the items grouped in a submenu, each kind gets a verb named after the menu, with `MUIVerb` (the title) and `SubCommands` empty, and the items it applies to as verbs in its own `shell` key. Explorer shows one verb of a name, taking an extension's over `*`'s, so the submenu under an extension holds the items for any file as well. Without a submenu, each item is a verb named `<menu>.<n>.<item>`. Windows orders verbs by their keys' names, so the items' keys start with their place in the menu (`00.extract`). Every verb of the menu's carries a value `ContextMenuRs` naming the menu: `uninstall` removes the keys that carry it, and nothing else. `install` writes the menu's definition too, in `%LOCALAPPDATA%\context-menu-rs\<menu>.menu` ([definition.md](definition.md)), for the native extension to come.

What the classic menu can and cannot do:

- **Windows runs the program once for each file chosen**, with that file's path. An item for several files at once (`Count::Many`) cannot get them in one run; an app that wants them together has to gather the runs itself, as a single instance. The others show for any number (`MultiSelectModel` `Player`, past Windows's limit of 15).
- **`Count::One` hides an item when more than one is chosen** (`MultiSelectModel` `Single`), but in a submenu only when all of a kind's items are `Count::One`: Windows reads the value for the submenu's own verb, not for those in it. Otherwise the item shows, and runs once for each.
- **With different things chosen**, files and folders or a zip and a text file, the menu is that of the one right-clicked, and the item chosen runs for every one of them, even those it is not for. An app has to check the paths it gets.
- **Windows knows a file's last extension only.** An item for `tar.gz` is a verb under `.gz` with an `AppliesTo` condition naming the longer extension (`System.FileName:~>".tar.gz"`), and so is a submenu when all its items have one, or it would show empty for other `.gz` files. Extensions match in any case.
- **Icons** are an `.ico`, or an `.exe` or `.dll` with an index (`C:\Program Files\App\app.exe,0`). An icon theme's name, for Linux, is left out.
- **The command** is the program and its arguments, each quoted as `CommandLineToArgvW` reads it back, and then `"%1"`. Windows replaces its own codes in it (`%1`, `%V`, `%L`, `%W`…), so an argument holding a `%` and a digit or one of those letters may change.

## Trying it

`cargo run --example demo -- install` puts the demo menu in, for files, archives and folders; choosing an item shows the paths in a message box, and writes them to `%TEMP%\context-menu-demo.log`. `uninstall` takes it away.

`scripts/windows/menu.ps1` prints the classic menu Explorer builds for some files, as Explorer asks the shell for it, submenus and all, and can choose an item, with no window: `.\menu.ps1 C:\Users\me\a.zip`, or `-Choose 'context-menu demo/Archives: show their paths'`.

`tests/windows.rs` writes the menu to a scratch key (`HKEY_CURRENT_USER\Software\context-menu-rs-test-…`), not the one Explorer reads, and reads it back: what is written, `is_current` before and after a change, a key changed by hand, and that other apps' verbs stay.
