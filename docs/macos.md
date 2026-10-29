# macOS

Finder's right-click menu takes items from two places: Quick Actions, which are Automator workflows in the user's Services folder, and Finder Sync extensions, which are app extensions inside an app's bundle. `install` writes Quick Actions, for any app, with no rights beyond the user's. A Finder Sync extension comes later (see "Not yet").

## Quick Actions

`install` writes a bundle for each item in `~/Library/Services`, named `<menu>.<item>.workflow`, as Automator saves a Quick Action:

- `Contents/Info.plist` declares the service: its label (`NSMenuItem`), for Finder alone (`NSRequiredContext`), and for the file types the item applies to (`NSSendFileTypes`). A key `ContextMenuRs` names the menu.
- `Contents/document.wflow` is the workflow: one action, Run Shell Script, which gets the paths chosen as its arguments and runs `set -m; '<program>' '<argument>'… "$@" </dev/null >/dev/null 2>&1 &` with `/bin/sh`. The program and its arguments are quoted for a shell, so the paths reach the program as they are, in one run. It is started and not waited for, as other file managers start it, so the Quick Action ends at once: with job control on (`set -m`), the program runs in a process group of its own, and outlives the one running the Quick Action; with its output going nowhere, Run Shell Script has nothing to wait for.

`install` first removes the bundles of an earlier install of the same menu that it does not write again, and writes each bundle in a hidden folder beside it first, so the system never reads half of one. `uninstall` removes the bundles named after the menu, and leaves every other one alone: ids hold no `.`, so another menu's names never match. Both then ask the system to read the Services folder again (`/System/Library/CoreServices/pbs -update`, as Automator does when it saves), without waiting for it, since it takes a few seconds.

The system leaves a new Quick Action off: Finder's Quick Actions submenu then holds only Customize…, where the user turns it on. So `install` turns on the menu's Quick Actions that the user has not turned on or off yet, writing what System Settings writes when they do: in the preferences `pbs`, under `NSServicesStatus`, a key `(null) - <label> - runWorkflowAsService`, on for Finder's right-click menu, its preview pane and the Services menu. The key goes by the label alone, so workflows with the same label share it. One the user turned off stays off, and `uninstall` leaves the keys, so installing again keeps the user's choice.

### File types

Finder chooses by file type (a UTI), not by name. `install` asks the system which type a file with each extension has, and puts that in `NSSendFileTypes`:

- **An extension an app or the system declares** has its type: `zip` is `public.zip-archive`, `7z` is `org.7-zip.7-zip-archive` when an app declares it. Types also declared for it, such as a package's, are added.
- **An extension nothing declares** gets a type made up for it, `dyn.…`, which is what files with it have.
- **Any file** is `public.data`, and `com.apple.package` for the folders Finder shows as files: apps, and documents that are packages.
- **Folders** are `public.folder`.

The system says this through `UTType`, from JavaScript for Automation (`osascript`), so the crate calls no Objective-C itself; `UTType` is macOS 11's. What the types are can change when an app declaring one is installed or removed, so `is_current` asks again, which takes about a tenth of a second, and is false then.

### What Quick Actions can and cannot do

- **They are in Finder's Quick Actions submenu**, with any other app's and the user's, and not grouped under the menu's title: `Menu::submenu` is not used, and the report's notes say so. Finder orders them by their labels, not as the menu has them.
- **The user can turn each one off**, in Quick Actions, Customize…, or in System Settings, under Keyboard, Keyboard Shortcuts, Services, Files and Folders, where they are listed.
- **They show for any number of files chosen.** An item for `Count::One` shows for several files too, and runs with all of them; one for `Count::Many` shows for one file. The notes say so for each such item, and an app has to check what it gets.
- **A file's type comes from its last extension.** An item for `tar.gz` shows for every `.gz` file. The notes say so.
- **With different things chosen, an item shows only when each one is of a type it is for**, as on Linux: with a zip and a 7z chosen an item for archives shows, but not with a zip and a text file, or a zip and a folder.
- **A type takes in those that conform to it**: `.jar`'s type is a kind of zip archive, and `.tgz`'s a kind of gzip file, so an item for `zip` shows for `.jar` files, and one for `gz` for `.tgz`.
- **Only in Finder**, and only for files with a path.
- **No icons**: the items have Automator's own.
- **The program's output goes nowhere**, since nothing waits for it.

## Trying it

`cargo run --example demo -- install` writes the demo's Quick Actions; right-click a file or a folder in Finder and open Quick Actions. Choosing an item shows the paths in an alert, and writes them to `context-menu-demo.log` in `$TMPDIR`. `uninstall` takes them away.

## What has been tried

The bundles are planned in plain Rust and tested on every system: the files written, the notes, installing again, `is_current` and `uninstall`. On macOS, `tests/macos.rs` writes them to a scratch folder, not `~/Library/Services`, with the types the system knows, and checks:

- `plutil -lint` reads both files of each bundle;
- `pbs -read_bundle`, the system's own reader, finds the service, with its label, Finder, and the types of `zip` and `gz`;
- `automator` runs the workflow as Finder would, with paths holding spaces, quotes and `$`, and the program gets them as they are, in one run.

That was on macOS 27.0.1, where the test writes the keys turning the Quick Actions on to a scratch property list, not `pbs`, and checks that one turned off stays off.

On that Mac, the demo was then installed for the user and tried in Finder. Its items first showed only once turned on in Customize…, which is why `install` turns them on; with that, and the keys cleared, they showed at once after `install`, and went at once after `uninstall`. They are labelled as `Info.plist` says, not with the bundles' names, and ordered by label: "Anything" before "Archives", though written after it. System Settings listed them. Each showed for what it applies to, as above: for a zip, a 7z or both, the archives' item and the one for anything, and for a `.jar` or a `.tgz` too; for a zip and a text file, or a zip and a folder, the one for anything alone. Several files chosen reached the program in one run, `it's a b.zip` as it is. When the Quick Action waited for the program, a gear turned in the menu bar until the demo's alert closed; so it does not wait any more, and then no gear showed, and the alert stayed open past a minute, until it was closed. The system's process running Quick Actions, `WorkflowServiceRunner`, was still there afterwards, so a program outliving it has not been seen: its own process group is for that.

## Not yet

- **A Finder Sync extension**, built into an app's bundle, would put the menu in Finder's menu itself, grouped, and show each item only for what it applies to, as on Linux and Windows 11. But Apple says such an extension is not meant to add a menu to every folder; it must run in the sandbox, so it cannot start the program itself and has to ask the app to; and it would have to read the menu's definition from a place the sandbox lets it reach. It needs trying on a virtual machine before it is decided on.
