# macOS

Finder's right-click menu takes items from two places: Finder Sync extensions, which are app extensions inside an app's bundle, and Quick Actions, which are Automator workflows in the user's Services folder. So there are two ways in:

- **With the native extension**, `context-menu-finder`, which an app's bundle holds, the menu is in Finder's own menu, right after Quick Actions, in a submenu of its own, showing exactly the items for what is chosen, as on Linux and Windows 11. `install` sees it and writes no Quick Actions.
- **`install` alone** writes Quick Actions, for any app, with no change to its bundle. They have the limits below.

Either way `install` needs no rights beyond the user's, and writes the menu's definition in `~/Library/Application Support/context-menu-rs/<menu>.menu` ([definition.md](definition.md)).

## The native extension

It is one program, `context-menu-finder`, built once for everybody's menus. An app's bundle holds it twice: as a Finder Sync extension, which Finder runs in the sandbox, and as its helper, an app with no window, which starts the items' programs, since the sandbox lets the extension start none.

```text
Photo Kit.app/Contents/MacOS/photokit
Photo Kit.app/Contents/PlugIns/ContextMenu.appex/Contents/MacOS/context-menu-finder
Photo Kit.app/Contents/Helpers/ContextMenuHelper.app/Contents/MacOS/context-menu-finder
```

What each is comes from its bundle: the extension's `Info.plist` has `NSExtension`, and both name the menu they serve (`ContextMenuRsMenu`) and the app group they share (`ContextMenuRsGroup`). So:

- **The extension reads the menu's definition** each time Finder asks what to show, which `install` writes for each user; a changed menu (the app's settings turning an item off, say) is `install` again, and Finder shows the change at the next right click. The sandbox lets it read that folder alone, with a temporary exception, read only. Its home there is the user's own, from the system's list of users: the sandbox points `HOME` at the extension's container.
- **It shows in every folder**: the startup disk's, under `/`, and every other volume's, which `/` does not take in, as they come and go.
- **An item chosen is a request**: the extension leaves the item's id and the paths chosen in the app group's folder, `~/Library/Group Containers/<group>/requests`, which only the app's own programs can write, and opens the helper. The system drops the arguments a sandboxed extension gives an app it opens, and a URL scheme would let any web page ask; the group's folder does neither.
- **The helper checks the request against the menu**: the item must be the menu's, and the paths absolute, there, and what the item applies to. It takes the program and its arguments from the definition, never from the request; starts it with the paths, in the first one's folder, in a process group of its own, without waiting; and ends. A request older than a minute is dropped.

`install` asks the system for the extension whose `Info.plist` names the menu (`pluginkit -m -p com.apple.FinderSync ContextMenuRsMenu=<menu>`). When there is one:

- It writes no Quick Actions, and removes those an earlier install wrote, so the menu does not show twice.
- It turns the extension on when the user has not turned it on or off (`pluginkit -e use`), as with Quick Actions: the system leaves a new extension off. One the user turned off, in System Settings, stays off, with no Quick Actions in its place.

When there is none, but the program calling `install` is in an app bundle holding the extension, it tells the system of it first: LaunchServices (`lsregister -f -R`), which PlugInKit needs, then PlugInKit (`pluginkit -a`). The system does this by itself for apps in the Applications folder; for others, such as the demo, `install` does. `is_current` is false when an extension serving the menu comes or goes, is in the app but unknown to the system, or has not been turned on or off. `uninstall` removes the definition, so the extension shows nothing; the extension stays the app's.

What the menu shows:

- **A submenu with the menu's title** and its items, or, without `Menu::submenu`, the items themselves, right after Quick Actions.
- **Each item for what it applies to, all of it** (`AppliesTo::accepts`): `Count::One` and `Count::Many` count what is chosen, `tar.gz` is told from `gz`, and with a zip and a folder chosen an item for archives does not show. The folders Finder shows as files, apps and documents that are packages, count as files. Nothing shows when no item applies.
- **One run with every path**, as everywhere.
- **No icons** yet.

### For packagers

Get the program from the [releases](https://github.com/hivepaas/context-menu-rs/releases), `context-menu-finder-macos`, for both Apple silicon and Intel, or build it:

```sh
cargo build --release -p context-menu-finder   # target/release/context-menu-finder
```

Copy it into the app's bundle twice, at `context_menu::macos::EXTENSION_PATH` and `HELPER_PATH`, each as `Contents/MacOS/context-menu-finder`. `context_menu::macos::FinderExtension::new(menu id, app's bundle id, team id, name)`, with `version` set to the app's, gives the rest:

- `extension_plist()` and `helper_plist()`, each bundle's `Contents/Info.plist`. The extension's bundle id is the app's and `.context-menu`, the helper's the app's and `.context-menu-helper`.
- `extension_entitlements()` and `helper_entitlements()`, to sign them with: the sandbox for the extension, the app group for both, `<team id>.<app's bundle id>.context-menu`, and reading the definitions for the extension. A group that starts with the team's id needs no provisioning profile; it does need the team's certificate, so the extension cannot be signed ad hoc.

Sign inside out, with the hardened runtime, then the app, and notarize it as any app:

```sh
codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  --entitlements helper.entitlements "Photo Kit.app/Contents/Helpers/ContextMenuHelper.app"
codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  --entitlements extension.entitlements "Photo Kit.app/Contents/PlugIns/ContextMenu.appex"
codesign --force --sign "$IDENTITY" --options runtime --timestamp "Photo Kit.app"
```

The app calls `install`, and `is_current` when it starts, from its own program, in its bundle, as on any other system.

### How it is made

`src/lib.rs` decides, in plain Rust tested on every system: what shows for the files chosen, the requests' text, leaving and taking them, and what a request runs. `src/ffi.rs` is the Objective-C side, through objc2, and the only unsafe code: the extension's principal class, `ContextMenuRsFinderSync`, a `FIFinderSync` that `NSExtensionMain` hands to Finder; the volumes watched; the menu; and the helper. Finder asks for the menu, and calls an item chosen, on one of the extension's threads, not the main one, and takes the menu there: it is built there, as Apple's own template builds it. Every method Finder calls catches a panic and shows nothing instead.

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

`cargo run --example demo -- install` writes the demo's Quick Actions; right-click a file or a folder in Finder and open Quick Actions. Choosing an item shows the paths in an alert, and writes them to `context-menu-demo.log` in `$TMPDIR`. `uninstall` takes them away, and `status` says whether `install` would change anything.

For the native extension, with a code-signing identity of your team's in the keychain:

```sh
IDENTITY='Developer ID Application: Name (TEAMID)' scripts/macos/demo-app.sh   # --remove takes it away
```

It builds the demo as an app holding the extension and its helper, `target/context-menu-demo/ContextMenuDemo.app`, with the `Info.plist`s and entitlements `demo bundle` writes, signs it, and runs `install` from inside it: `install` tells the system of the extension, turns it on, and writes no Quick Actions. The demo's menu is then a submenu, "context-menu demo", in Finder's own menu.

## What has been tried

### The native extension

On macOS 27.0.1, on Apple silicon, the demo app was built and signed with a Developer ID and the hardened runtime, installed for the user and tried in Finder. `install` told the system of the extension, outside the Applications folder, and turned it on, with no prompt; the demo's Quick Actions were gone. In Finder:

| Chosen | The submenu held |
|---|---|
| a zip, a `.tar.gz` | Archives, Anything |
| a `.gz`, a text file | Anything |
| a folder | Anything, One folder |
| two folders | Anything |
| a zip and a text file | Anything |

Choosing an item ran the demo with the paths, through a request in the app group's folder, with no prompt. Earlier trials, with the same design, showed it on the Desktop, in Downloads, in iCloud Drive and on a USB drive (NTFS), once every volume was watched; that the arguments of a sandboxed extension to the app it opens are dropped; and that Finder asks for the menu off the main thread, which shows no menu when the extension insists on the main one.

The app built again at the same place, the system took the new extension by itself, still on; when a trial's extension was lost to the system instead, telling the system again, LaunchServices first, brought it back, still on.

Not tried yet: notarizing an app with the extension, an Intel Mac, and macOS before 27.

### Quick Actions

The bundles are planned in plain Rust and tested on every system: the files written, the notes, installing again, `is_current` and `uninstall`. On macOS, `tests/macos.rs` writes them to a scratch folder, not `~/Library/Services`, with the types the system knows, and checks:

- `plutil -lint` reads both files of each bundle;
- `pbs -read_bundle`, the system's own reader, finds the service, with its label, Finder, and the types of `zip` and `gz`;
- `automator` runs the workflow as Finder would, with paths holding spaces, quotes and `$`, and the program gets them as they are, in one run.

That was on macOS 27.0.1, where the test writes the keys turning the Quick Actions on to a scratch property list, not `pbs`, and checks that one turned off stays off.

On that Mac, the demo was then installed for the user and tried in Finder. Its items first showed only once turned on in Customize…, which is why `install` turns them on; with that, and the keys cleared, they showed at once after `install`, and went at once after `uninstall`. They are labelled as `Info.plist` says, not with the bundles' names, and ordered by label: "Anything" before "Archives", though written after it. System Settings listed them. Each showed for what it applies to, as above: for a zip, a 7z or both, the archives' item and the one for anything, and for a `.jar` or a `.tgz` too; for a zip and a text file, or a zip and a folder, the one for anything alone. Several files chosen reached the program in one run, `it's a b.zip` as it is. When the Quick Action waited for the program, a gear turned in the menu bar until the demo's alert closed; so it does not wait any more, and then no gear showed, and the alert stayed open past a minute, until it was closed. The system's process running Quick Actions, `WorkflowServiceRunner`, was still there afterwards, so a program outliving it has not been seen: its own process group is for that.

## Not yet

- **Icons** in the extension's menu.
- **The menu of a folder's background**, which Finder asks the extension for too: as on Linux and Windows, items are for what is chosen.
- Apple says a Finder Sync extension is meant for folders an app syncs, not every folder; this one watches every folder, as other apps' do, and that has worked so far.
