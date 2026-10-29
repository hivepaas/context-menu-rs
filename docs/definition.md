# The menu definition

`install` writes each menu as text, for the native extensions to read when the file manager asks what to show: on Linux in `$XDG_DATA_HOME/context-menu-rs/<menu id>.menu` (by default `~/.local/share/context-menu-rs/`), on Windows in `%LOCALAPPDATA%\context-menu-rs\`, and on macOS in `~/Library/Application Support/context-menu-rs/`. `Menu::to_definition` and `Menu::from_definition` write and read it; an app not written in Rust can write it too, and use the extensions.

The first line names the format and its version:

```text
context-menu-rs menu 1
```

A reader refuses another version. Every other line is a key, a tab and a value; empty lines are passed over, and so are keys a reader does not know, so a newer writer's additions do not stop an older reader. In values, a backslash is written `\\`, a tab `\t` and a line break `\n`.

The menu's fields come first. A line `item` starts an item, with its id, and the fields after it are that item's, until the next `item`.

| Key | Of | Value |
|---|---|---|
| `id` | the menu | Its id: 1 to 64 lower-case ASCII letters, digits and inner `-` |
| `title` | the menu | The submenu's label |
| `icon` | the menu, an item | An icon, as `Item::icon` has it for the system: an icon theme's or a symbol's name, or a path. On macOS, `install` writes the path of the copy it makes of an image file, which the sandboxed extension can read |
| `submenu` | the menu | `true` or `false`: the items grouped under one entry |
| `item` | | Starts an item; its id, as the menu's |
| `label` | an item | What the menu says |
| `files` | an item | `none`, `any`, or `extensions`, for the `extension` lines after it |
| `extension` | an item | One extension, without the dot, in lower case: `zip`, `tar.gz` |
| `folders` | an item | `true` or `false`: folders too |
| `count` | an item | `one`, `many` (two or more) or `any` (one or more) |
| `program` | an item | The program to run, an absolute path |
| `arg` | an item | One argument, before the paths chosen; one line each, in order |

The menu read is checked as `Menu::validate` checks one: ids, labels, an absolute program, at least one item.

```text
context-menu-rs menu 1
id	photokit
title	PhotoKit
submenu	true
item	resize
label	Resize to 1080p
files	extensions
extension	jpg
extension	png
folders	false
count	any
program	/usr/bin/photokit
arg	--resize
arg	1080
```

An item applies when the files and folders chosen are as many as `count` says, and each one is a folder (with `folders` true) or a file `files` allows: any file, or one whose name ends in `.` and one of its extensions, in any case. A file with no local path, such as one on an SFTP server, gets no items. The program runs with the item's arguments and then the paths chosen.
