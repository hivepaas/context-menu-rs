# Linux

There is no one right-click menu on Linux: each file manager has its own way in. `install` writes the menu for each of the file managers below, whether or not it is installed, so the menu is there when one is. Every file goes in the user's data folder (`$XDG_DATA_HOME`, by default `~/.local/share`), or for Thunar in the user's config folder (`$XDG_CONFIG_HOME`, by default `~/.config`), never in the system's, so no root is needed.

Each file starts with a comment naming the menu. `uninstall` finds the files by their names and by that comment, and leaves everything else alone. `install` first removes what an earlier install of the same menu wrote, so installing a changed menu leaves nothing stale.

Every item runs its program with its arguments and then the paths chosen, in one run. Only local files have paths, so items show for local files only. The menu's definition goes in `context-menu-rs/<menu>.menu` too, for the native extensions ([definition.md](definition.md)).

| File manager | Where | Grouped in a submenu | Only for the files it applies to | Icons |
|---|---|---|---|---|
| Nautilus, with the app's native extension | The definition; the extension is the package's ([nautilus.md](nautilus.md)) | Yes | Yes | No (GTK 4 menus show none) |
| Nautilus, with nautilus-python | `nautilus-python/extensions/<menu>_context_menu.py` | Yes | Yes | No |
| Nautilus, with neither | `nautilus/scripts/[<title>/]<label>` | Under Scripts, and the title | No | No |
| Dolphin (KDE) | `kio/servicemenus/<menu>.<item>.desktop` | Yes | Yes, by MIME type | Yes |
| Nemo (Cinnamon) | `nemo/actions/<menu>.<item>.nemo_action` | No | Yes | Theme names only |
| Thunar (Xfce) | Its custom actions, `~/.config/Thunar/uca.xml`, shared with the user's own | Yes | Yes, but extensions in lower or upper case only | Yes |

## Nautilus (GNOME Files)

Nautilus takes menu items only from extensions: libraries in one folder of the system's, fixed when Nautilus was built, so putting one there takes root. `install` chooses the best of three ways the system offers, in this order.

The app's package installed the native extension, `libcontext-menu-<menu id>.so`, in Nautilus's extension folder: it reads the menu's definition, and `install` writes nothing more for Nautilus. [nautilus.md](nautilus.md) is for packagers.

Else, with nautilus-python installed (`python3-nautilus` on Debian and Ubuntu, `nautilus-python` on Fedora and Arch), the menu is a Python extension written for its API 4.0, the one of Nautilus 43 and later. It holds the menu as data, decides what to show from the files chosen, and runs the program through `Gio.Subprocess`. Nautilus loads extensions when it starts: `nautilus -q` quits it, so the next window loads it.

Without nautilus-python, the only way in is Nautilus's scripts: an executable file per item, named after its label, under the Scripts submenu (and in a folder named after the menu's title, when the items are grouped). Scripts show for any file chosen, whatever the item applies to: the program should check what it gets. The script takes the paths from `NAUTILUS_SCRIPT_SELECTED_FILE_PATHS`, one per line, so a path with a line break in its name is split.

Which way is decided at install time, by looking in Nautilus's extension folders for the menu's library and for `libnautilus-python.so`. Installing again after either is added or removed changes the way: see below.

## Dolphin (KDE)

A service menu per item, in `kio/servicemenus`, the folder KDE Frameworks 5.85 and later read; KIO reads those in the user's folder only when they are executable, so they are. An item is a file of its own because what a service menu shows for (its MIME types, how many files) is set per file; files with the same `X-KDE-Submenu` share the submenu, and `X-KDE-Priority=TopLevel` puts them in the menu itself rather than its Actions submenu. Dolphin reads the files again when they change.

Dolphin chooses by MIME type, not by name. `install` looks each extension up in shared-mime-info's database (`mime/globs2` in each data folder); an extension it does not know there is left out, and the report's notes say so. Any file is `application/octet-stream`, which every type is a kind of; folders are `inode/directory`.

`desktop-file-validate` reports `MimeType` and `Actions` as errors in a `Type=Service` file: service menus are KDE's extension of the Desktop Entry specification, which knows only applications, links and directories.

## Nemo (Cinnamon)

An action per item, in `nemo/actions`. Nemo matches `Extensions` against the end of a file's name, with no dot of its own, so each extension is written with its dot (`.zip`; `zip` would also match a file named `unzip`), which also makes `.tar.gz` work. Its command line is split as a shell would split it, so the program and its arguments are quoted for a shell. Nemo has no way for an action to put itself in a submenu: its users group actions in Nemo's own Actions layout. Its icons are theme names only, so an icon given as a path is left out.

## Thunar (Xfce)

Thunar keeps every custom action, the user's own and those of apps, in one file, `Thunar/uca.xml` in the user's config folder; with none there, it reads the system's (`/etc/xdg/Thunar/uca.xml`), which holds a sample action, "Open Terminal Here". So `install` merges the menu's actions into the user's file, written as Thunar writes them (`thunar-uca-model.c`), and leaves every other action as it is; their unique ids, `context-menu-rs.<menu>.<item>`, tell the menu's apart. The first time, it starts the user's file from the system's, so its actions do not disappear. A file in another shape than Thunar writes (an action with attributes, say) is left alone, and the report's notes say so. Thunar's own editor keeps the actions, unique ids and all, when it saves.

Thunar reads the file when it starts, and never again: `thunar -q` quits it, so that the next window shows the change.

What Thunar can say of an action shapes what is written:

- Its patterns match a name minding case, so each extension is written in lower case and in upper case (`*.zip;*.ZIP`): `A.Zip` is not matched.
- Its patterns apply to folders' names too, so an item for some extensions and for folders is two actions, one for the files and one, `<item>.folders`, for folders; a choice of both shows neither.
- How many files may be chosen is a range of numbers: `1-1` for one, `2-1000000` for two or more.
- Its command line gets the paths for `%F`, quoted for a shell, and is split as a shell splits it, so the program and its arguments are quoted for a shell. An argument holding Thunar's own codes (`%f`, `%F`, `%u`, `%U`, `%d`, `%D`, `%n`, `%N`) has them replaced: Thunar has no way to write them literally.

## Keeping the menu current

What `install` writes depends on the system as well as the menu: nautilus-python or a package's native extension may be added or removed after it. `is_current(&menu)` says whether what is installed is what `install` would write now, file for file, so an app can call it when it starts and `install` when it is false.

## For an app's packages

- A package for a distribution can put the native extension in Nautilus's folder ([nautilus.md](nautilus.md)), so its users get the menu at its best without nautilus-python. Short of that, recommending nautilus-python (`Recommends: python3-nautilus` on Debian and Ubuntu, `nautilus-python` on Fedora) gets the Python extension.
- Opening a file with the app is not the right-click menu's job: a desktop entry with `MimeType=` puts the app in every file manager's Open With, with no extension.
- A Flatpak's or a Snap's data folder is its own (for a Flatpak, `~/.var/app/<app id>/data`), where no file manager looks, so by default `install` writes where nothing reads it. A Flatpak needs permission to write the file managers' folders (such as `--filesystem=xdg-data/kio/servicemenus`, `xdg-data/nemo/actions` and `xdg-data/nautilus`) and `XDG_DATA_HOME` pointed back at the user's; the native extension, in the system's folders, is out of its reach.

## Not yet

- Caja (MATE), PCManFM and Konqueror's older KF5 folder (`kservices5/ServiceMenus`, before 5.85).

## What has been tried

The native extension is loaded as Nautilus loads it, with Nautilus's own library (`libnautilus-extension.so.4`, Nautilus 50.2 on GNOME 50), asked what it shows for choices of files, and an item chosen: see [nautilus.md](nautilus.md). The other files are tested against what each file manager's documentation and source say: the Python extension is run by Python with stand-ins for Nautilus's classes, and the scripts by `sh` with awkward paths in `NAUTILUS_SCRIPT_SELECTED_FILE_PATHS`, as Nautilus runs them. Dolphin's files pass `desktop-file-validate` but for the two errors above, and the extensions find their MIME types in a real shared-mime-info database. Thunar's file is written as Thunar writes it, checked against its source, and read back by an XML parser. None of the menus has been clicked in a running Nautilus, Dolphin, Nemo or Thunar yet.
