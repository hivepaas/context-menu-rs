//! Nautilus (GNOME Files). With nautilus-python installed, an extension
//! puts the items in the menu itself, for the files they apply to. Without
//! it, the only way in is a script per item in Nautilus's Scripts submenu,
//! which shows for every file chosen.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::quote::{python, python_list, shell};
use super::{Dirs, File, list, mark};
use crate::{Count, Error, Files, Item, Menu, Report};

/// Where nautilus-python's library may be: Debian's multiarch folders,
/// Fedora's `lib64`, Arch's `lib`.
pub(super) fn has_python() -> bool {
    let tail = "nautilus/extensions-4/libnautilus-python.so";
    let mut libs = vec![PathBuf::from("/usr/lib64"), PathBuf::from("/usr/lib")];
    if let Ok(entries) = std::fs::read_dir("/usr/lib") {
        libs.extend(
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.to_string_lossy().contains("-linux-")),
        );
    }
    libs.iter().any(|l| l.join(tail).exists())
}

fn extensions_dir(dirs: &Dirs) -> PathBuf {
    dirs.data_home.join("nautilus-python/extensions")
}

fn scripts_dir(dirs: &Dirs) -> PathBuf {
    dirs.data_home.join("nautilus/scripts")
}

/// The extension's module name: ids hold `-`, which Python names cannot.
fn module(id: &str) -> String {
    format!("{}_context_menu", id.replace('-', "_"))
}

pub(super) fn files(menu: &Menu, dirs: &Dirs, out: &mut Vec<File>, notes: &mut Vec<String>) {
    if dirs.nautilus_python {
        out.push(File {
            path: extensions_dir(dirs).join(format!("{}.py", module(&menu.id))),
            text: extension(menu),
            executable: false,
        });
        notes.push("Nautilus: restart it (nautilus -q) for the menu to show.".into());
        return;
    }
    let mut folder = scripts_dir(dirs);
    if menu.submenu {
        folder.push(file_name(&menu.title));
    }
    let mut taken: Vec<String> = Vec::new();
    for item in &menu.items {
        let mut name = file_name(&item.label);
        if taken.contains(&name) {
            name = file_name(&format!("{} ({})", item.label, item.id));
        }
        taken.push(name.clone());
        out.push(File {
            path: folder.join(&name),
            text: script(menu, item),
            executable: true,
        });
    }
    notes.push(
        "Nautilus: without nautilus-python the items are in the Scripts submenu, for any \
         file chosen; with it (python3-nautilus, nautilus-python) they show in the menu itself."
            .into(),
    );
}

pub(super) fn remove(id: &str, dirs: &Dirs, report: &mut Report) -> Result<(), Error> {
    let name = module(id);
    super::remove(&extensions_dir(dirs).join(format!("{name}.py")), report)?;
    // What Python compiled of it.
    let cache = extensions_dir(dirs).join("__pycache__");
    for path in list(&cache)? {
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(&format!("{name}.")))
        {
            super::remove(&path, report)?;
        }
    }
    // Scripts, found by their mark, in the folder and a submenu's; a
    // submenu's folder goes once it is empty.
    let mark_line = format!("# {}", mark(id));
    let scripts = scripts_dir(dirs);
    for path in list(&scripts)? {
        if path.is_dir() {
            let before = report.removed.len();
            for inner in list(&path)? {
                remove_marked(&inner, &mark_line, report)?;
            }
            if report.removed.len() > before && list(&path)?.is_empty() {
                std::fs::remove_dir(&path).map_err(|e| Error::io(&path, e))?;
            }
        } else {
            remove_marked(&path, &mark_line, report)?;
        }
    }
    Ok(())
}

/// Removes `path` if it is a script holding `mark_line`.
fn remove_marked(path: &Path, mark_line: &str, report: &mut Report) -> Result<(), Error> {
    if !path.is_file() {
        return Ok(());
    }
    // Not text, or unreadable: not one of ours.
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    if text.lines().any(|l| l == mark_line) {
        super::remove(path, report)?;
    }
    Ok(())
}

/// A label as a file's name, which Nautilus shows: no `/`, not hidden.
fn file_name(label: &str) -> String {
    let name = label.replace('/', "\u{2215}");
    match name.strip_prefix('.') {
        Some(rest) => format!("\u{2024}{rest}"),
        None => name,
    }
}

fn script(menu: &Menu, item: &Item) -> String {
    let mut command = shell(&item.program.to_string_lossy());
    for arg in &item.args {
        command.push(' ');
        command.push_str(&shell(arg));
    }
    format!(
        "#!/bin/sh\n\
         # {}\n\
         # Nautilus gives the paths of the files chosen, one per line; only local\n\
         # files have one.\n\
         [ -n \"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\" ] || exit 0\n\
         IFS='\n'\n\
         set -f\n\
         exec {command} $NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\n",
        mark(&menu.id)
    )
}

/// The extension, with the menu written into it as data.
fn extension(menu: &Menu) -> String {
    let mut items = String::new();
    for item in &menu.items {
        let a = &item.applies_to;
        let (files, extensions): (&str, Vec<String>) = match &a.files {
            Files::None => ("none", Vec::new()),
            Files::Any => ("any", Vec::new()),
            Files::Extensions(list) => {
                ("extensions", list.iter().map(|e| format!(".{e}")).collect())
            }
        };
        let count = match a.count {
            Count::One => "one",
            Count::Many => "many",
            Count::Any => "any",
        };
        let program = item.program.to_string_lossy();
        let argv = std::iter::once(program.as_ref()).chain(item.args.iter().map(String::as_str));
        let _ = write!(
            items,
            "    {{\n\
             \x20       'id': {},\n\
             \x20       'label': {},\n\
             \x20       'files': '{files}',\n\
             \x20       'extensions': {},\n\
             \x20       'folders': {},\n\
             \x20       'count': '{count}',\n\
             \x20       'argv': {},\n\
             \x20   }},\n",
            python(&item.id),
            python(&item.label),
            python_list(&extensions),
            if a.folders { "True" } else { "False" },
            python_list(argv),
        );
    }
    format!(
        "# {mark}\n\
         # A Nautilus extension for nautilus-python, API 4.0.\n\
         \n\
         from gi.repository import Gio, GLib, GObject, Nautilus\n\
         \n\
         MENU = {id}\n\
         TITLE = {title}\n\
         SUBMENU = {submenu}\n\
         ITEMS = [\n\
         {items}]\n\
         {body}",
        mark = mark(&menu.id),
        id = python(&menu.id),
        title = python(&menu.title),
        submenu = if menu.submenu { "True" } else { "False" },
        body = EXTENSION_BODY.replace("{class}", &format!("ContextMenu_{}", module(&menu.id))),
    )
}

/// The extension's code, the same for every menu.
const EXTENSION_BODY: &str = r#"

def _applies(item, files):
    count = len(files)
    if count == 0:
        return False
    if item['count'] == 'one' and count != 1:
        return False
    if item['count'] == 'many' and count < 2:
        return False
    for f in files:
        if f.get_uri_scheme() != 'file':
            return False
        if f.is_directory():
            if not item['folders']:
                return False
        elif item['files'] == 'none':
            return False
        elif item['files'] == 'extensions':
            name = f.get_name().lower()
            if not any(name.endswith(e) for e in item['extensions']):
                return False
    return True


def _run(_entry, argv, files):
    paths = [f.get_location().get_path() for f in files]
    try:
        Gio.Subprocess.new(argv + paths, Gio.SubprocessFlags.NONE)
    except GLib.Error as e:
        print('context-menu-rs:', MENU, e.message)


class {class}(GObject.GObject, Nautilus.MenuProvider):
    def get_file_items(self, files):
        files = list(files)
        entries = []
        for item in ITEMS:
            if _applies(item, files):
                entry = Nautilus.MenuItem(
                    name='context-menu-rs::' + MENU + '::' + item['id'],
                    label=item['label'],
                )
                entry.connect('activate', _run, item['argv'], files)
                entries.append(entry)
        if SUBMENU and entries:
            top = Nautilus.MenuItem(name='context-menu-rs::' + MENU, label=TITLE)
            submenu = Nautilus.Menu()
            for entry in entries:
                submenu.append_item(entry)
            top.set_submenu(submenu)
            return [top]
        return entries

    def get_background_items(self, current_folder):
        return []
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::testing::scratch;
    use std::process::Command;

    fn has(program: &str) -> bool {
        Command::new(program).arg("--version").output().is_ok()
    }

    /// A program that writes its arguments to `$OUT`, one per line.
    fn recorder(dir: &Path) -> PathBuf {
        let path = dir.join("record");
        std::fs::write(
            &path,
            "#!/bin/sh\nfor a; do printf '%s\\n' \"$a\"; done > \"$OUT\"\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn scripts_pass_the_paths_as_they_are() {
        if !has("sh") {
            return;
        }
        let dir = scratch("script");
        let item = Item::new("x", "X", recorder(&dir)).args(["it's", "$HOME", "a  b"]);
        let path = dir.join("script");
        std::fs::write(&path, script(&Menu::new("m", "M"), &item)).unwrap();
        let out = dir.join("out");
        let ok = Command::new("sh")
            .arg(&path)
            .env("OUT", &out)
            .env(
                "NAUTILUS_SCRIPT_SELECTED_FILE_PATHS",
                "/tmp/a b/c*\n/tmp/d'e\n",
            )
            .status()
            .unwrap()
            .success();
        assert!(ok);
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "it's\n$HOME\na  b\n/tmp/a b/c*\n/tmp/d'e\n"
        );
    }

    #[test]
    fn script_names_are_labels() {
        assert_eq!(file_name("Extract here"), "Extract here");
        assert_eq!(file_name("Zip/Unzip"), "Zip\u{2215}Unzip");
        assert_eq!(file_name(".hidden"), "\u{2024}hidden");
    }

    /// The extension, run by Python with stand-ins for Nautilus's classes:
    /// what it shows for each choice of files, and what it runs.
    #[test]
    fn the_extension_shows_items_for_what_they_apply_to() {
        if !has("python3") {
            return;
        }
        let menu = Menu::new("pack-wing", "Pack Wing")
            .submenu(true)
            .item(
                Item::new("extract", "Extract “here”", "/usr/bin/packwing")
                    .args(["--extract", "it's"])
                    .extensions(["zip", "tar.gz"]),
            )
            .item(
                Item::new("folder", "Pack folder", "/usr/bin/packwing")
                    .folders_only()
                    .count(Count::One),
            )
            .item(Item::new("two", "Compare", "/usr/bin/packwing").count(Count::Many));
        let dir = scratch("extension");
        let ext = dir.join("ext.py");
        std::fs::write(&ext, extension(&menu)).unwrap();
        let harness = dir.join("harness.py");
        std::fs::write(&harness, HARNESS).unwrap();
        let out = Command::new("python3")
            .arg("-I")
            .arg(&harness)
            .arg(&ext)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "a.ZIP: Pack Wing > Extract “here”\n\
             a.tar.gz b.zip: Pack Wing > Extract “here”, Compare\n\
             dir/: Pack Wing > Pack folder\n\
             dir/ a.zip: \n\
             a.txt: \n\
             remote.zip: \n\
             run: /usr/bin/packwing --extract it's /home/u/a.ZIP\n"
        );
    }

    const HARNESS: &str = r#"
import importlib.util, sys, types

class MenuItem:
    def __init__(self, name, label):
        self.name, self.label, self.submenu, self.handlers = name, label, None, []
    def connect(self, signal, cb, *args):
        self.handlers.append((cb, args))
    def set_submenu(self, menu):
        self.submenu = menu
class Menu:
    def __init__(self):
        self.items = []
    def append_item(self, item):
        self.items.append(item)
ran = []
class Subprocess:
    @staticmethod
    def new(argv, flags):
        ran.append(argv)
gi = types.ModuleType('gi'); repo = types.ModuleType('gi.repository')
repo.Nautilus = types.SimpleNamespace(MenuItem=MenuItem, Menu=Menu, MenuProvider=object)
repo.GObject = types.SimpleNamespace(GObject=type('GObject', (), {}))
repo.Gio = types.SimpleNamespace(Subprocess=Subprocess, SubprocessFlags=types.SimpleNamespace(NONE=0))
repo.GLib = types.SimpleNamespace(Error=Exception)
gi.repository = repo
sys.modules['gi'] = gi; sys.modules['gi.repository'] = repo

spec = importlib.util.spec_from_file_location('ext', sys.argv[1])
ext = importlib.util.module_from_spec(spec); spec.loader.exec_module(ext)
provider = next(v for k, v in vars(ext).items() if k.startswith('ContextMenu_'))()

class File:
    def __init__(self, name, scheme='file'):
        self.name, self.scheme = name.rstrip('/'), scheme
        self.dir = name.endswith('/')
    def get_uri_scheme(self): return self.scheme
    def is_directory(self): return self.dir
    def get_name(self): return self.name
    def get_location(self): return types.SimpleNamespace(get_path=lambda: '/home/u/' + self.name)

def show(*names, scheme='file'):
    entries = provider.get_file_items([File(n, scheme) for n in names])
    shown = []
    for e in entries:
        inner = e.submenu.items if e.submenu else []
        shown.append(e.label + (' > ' + ', '.join(i.label for i in inner) if inner else ''))
    print(' '.join(names) + ': ' + '; '.join(shown))
    return entries

first = show('a.ZIP')
show('a.tar.gz', 'b.zip')
show('dir/')
show('dir/', 'a.zip')
show('a.txt')
show('remote.zip', scheme='sftp')
entry = first[0].submenu.items[0]
cb, args = entry.handlers[0]
cb(entry, *args)
print('run: ' + ' '.join(ran[0]))
"#;
}
