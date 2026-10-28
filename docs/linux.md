# Linux

There is no one right-click menu on Linux: each file manager has its own way in. `install` writes the menu for each of the file managers below, whether or not it is installed, so the menu is there when one is. Every file goes in the user's data folder (`$XDG_DATA_HOME`, by default `~/.local/share`), never in the system's, so no root is needed.

Each file starts with a comment naming the menu. `uninstall` finds the files by their names and by that comment, and leaves everything else alone. `install` first removes what an earlier install of the same menu wrote, so installing a changed menu leaves nothing stale.

Every item runs its program with its arguments and then the paths chosen, in one run. Only local files have paths, so items show for local files only.

| File manager | Where | Grouped in a submenu | Only for the files it applies to | Icons |
|---|---|---|---|---|
| Nautilus, with nautilus-python | `nautilus-python/extensions/<menu>_context_menu.py` | Yes | Yes | No (GTK 4 menus show none) |
| Nautilus, without it | `nautilus/scripts/[<title>/]<label>` | Under Scripts, and the title | No | No |
| Dolphin (KDE) | `kio/servicemenus/<menu>.<item>.desktop` | Yes | Yes, by MIME type | Yes |
| Nemo (Cinnamon) | `nemo/actions/<menu>.<item>.nemo_action` | No | Yes | Theme names only |

## Nautilus (GNOME Files)

Nautilus takes menu items only from extensions, which are C libraries in the system's folders, or Python ones through nautilus-python. With nautilus-python installed (`python3-nautilus` on Debian and Ubuntu, `nautilus-python` on Fedora and Arch), the menu is a Python extension written for its API 4.0, the one of Nautilus 43 and later. It holds the menu as data, decides what to show from the files chosen, and runs the program through `Gio.Subprocess`. Nautilus loads extensions when it starts: `nautilus -q` quits it, so the next window loads it.

Without nautilus-python, the only way in is Nautilus's scripts: an executable file per item, named after its label, under the Scripts submenu (and in a folder named after the menu's title, when the items are grouped). Scripts show for any file chosen, whatever the item applies to: the program should check what it gets. The script takes the paths from `NAUTILUS_SCRIPT_SELECTED_FILE_PATHS`, one per line, so a path with a line break in its name is split.

Which of the two is decided at install time, by looking for `libnautilus-python.so` in the system's library folders. Installing again after nautilus-python is added replaces the scripts with the extension.

## Dolphin (KDE)

A service menu per item, in `kio/servicemenus`, the folder KDE Frameworks 5.85 and later read; KIO reads those in the user's folder only when they are executable, so they are. An item is a file of its own because what a service menu shows for (its MIME types, how many files) is set per file; files with the same `X-KDE-Submenu` share the submenu, and `X-KDE-Priority=TopLevel` puts them in the menu itself rather than its Actions submenu. Dolphin reads the files again when they change.

Dolphin chooses by MIME type, not by name. `install` looks each extension up in shared-mime-info's database (`mime/globs2` in each data folder); an extension it does not know there is left out, and the report's notes say so. Any file is `application/octet-stream`, which every type is a kind of; folders are `inode/directory`.

`desktop-file-validate` reports `MimeType` and `Actions` as errors in a `Type=Service` file: service menus are KDE's extension of the Desktop Entry specification, which knows only applications, links and directories.

## Nemo (Cinnamon)

An action per item, in `nemo/actions`. Nemo matches `Extensions` against the end of a file's name, with no dot of its own, so each extension is written with its dot (`.zip`; `zip` would also match a file named `unzip`), which also makes `.tar.gz` work. Its command line is split as a shell would split it, so the program and its arguments are quoted for a shell. Nemo has no way for an action to put itself in a submenu: its users group actions in Nemo's own Actions layout. Its icons are theme names only, so an icon given as a path is left out.

## Not yet

- Thunar (Xfce): its custom actions share one file, `~/.config/Thunar/uca.xml`, with the user's own, so it needs reading and merging XML.
- Caja (MATE), PCManFM and Konqueror's older KF5 folder (`kservices5/ServiceMenus`, before 5.85).

## What has been tried

The files are tested against what each file manager's documentation and source say: the extension is run by Python with stand-ins for Nautilus's classes, and the scripts by `sh` with awkward paths in `NAUTILUS_SCRIPT_SELECTED_FILE_PATHS`, as Nautilus runs them. Dolphin's files pass `desktop-file-validate` but for the two errors above, and the extensions find their MIME types in a real shared-mime-info database. None of the menus has been clicked in a running Nautilus, Dolphin or Nemo yet.
