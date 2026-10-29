//! The extension, loaded as Explorer loads it: renamed for its menu, asked
//! through COM what it shows for files and folders made for the test, and
//! an item chosen. Explorer's side of COM is written out here.

#![cfg(windows)]
#![allow(unsafe_code)]
// A test: failing loudly is its job, in the helpers too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use context_menu::{Count, Item, Menu, windows};

type Hresult = i32;
type Raw = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

impl Guid {
    /// From `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX`.
    fn parse(text: &str) -> Guid {
        let hex: String = text.chars().filter(|c| *c != '-').collect();
        let byte = |i: usize| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        Guid {
            data1: u32::from_str_radix(&hex[0..8], 16).unwrap(),
            data2: u16::from_str_radix(&hex[8..12], 16).unwrap(),
            data3: u16::from_str_radix(&hex[12..16], 16).unwrap(),
            data4: std::array::from_fn(|i| byte(8 + i)),
        }
    }
}

const IID_ICLASSFACTORY: &str = "00000001-0000-0000-C000-000000000046";
const IID_IEXPLORERCOMMAND: &str = "A08CE4D0-FA25-44AB-B57C-C7B1C323E0B9";
const S_OK: Hresult = 0;
const ECS_ENABLED: u32 = 0;
const ECS_HIDDEN: u32 = 2;
const ECF_HASSUBCOMMANDS: u32 = 1;
const COINIT_APARTMENTTHREADED: u32 = 2;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> Raw;
    fn GetProcAddress(module: Raw, name: *const u8) -> Raw;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: Raw, flags: u32) -> Hresult;
    fn CoTaskMemFree(memory: Raw);
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHParseDisplayName(
        name: *const u16,
        context: Raw,
        pidl: *mut Raw,
        want: u32,
        got: *mut u32,
    ) -> Hresult;
    fn SHCreateShellItemArrayFromIDLists(count: u32, pidls: *const Raw, out: *mut Raw) -> Hresult;
}

#[repr(C)]
struct FactoryVtbl {
    _query_interface: usize,
    _add_ref: usize,
    release: unsafe extern "system" fn(Raw) -> u32,
    create_instance: unsafe extern "system" fn(Raw, Raw, *const Guid, *mut Raw) -> Hresult,
}

#[repr(C)]
struct CommandVtbl {
    _query_interface: usize,
    _add_ref: usize,
    release: unsafe extern "system" fn(Raw) -> u32,
    get_title: unsafe extern "system" fn(Raw, Raw, *mut *mut u16) -> Hresult,
    get_icon: unsafe extern "system" fn(Raw, Raw, *mut *mut u16) -> Hresult,
    _get_tool_tip: usize,
    _get_canonical_name: usize,
    get_state: unsafe extern "system" fn(Raw, Raw, i32, *mut u32) -> Hresult,
    invoke: unsafe extern "system" fn(Raw, Raw, Raw) -> Hresult,
    get_flags: unsafe extern "system" fn(Raw, *mut u32) -> Hresult,
    enum_sub_commands: unsafe extern "system" fn(Raw, *mut Raw) -> Hresult,
}

#[repr(C)]
struct CommandsVtbl {
    _query_interface: usize,
    _add_ref: usize,
    release: unsafe extern "system" fn(Raw) -> u32,
    next: unsafe extern "system" fn(Raw, u32, *mut Raw, *mut u32) -> Hresult,
}

#[repr(C)]
struct UnknownVtbl {
    _query_interface: usize,
    _add_ref: usize,
    release: unsafe extern "system" fn(Raw) -> u32,
}

/// The vtable of the interface at `this`.
unsafe fn vtbl<'a, V>(this: Raw) -> &'a V {
    unsafe { &**this.cast::<*const V>() }
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain([0]).collect()
}

/// A string COM handed out, freed.
unsafe fn take(text: *mut u16) -> String {
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    let s = OsString::from_wide(unsafe { std::slice::from_raw_parts(text, len) });
    unsafe { CoTaskMemFree(text.cast()) };
    s.into_string().unwrap()
}

/// An `IShellItemArray` of `paths`, as Explorer hands the files chosen.
struct Chosen(Raw);

impl Chosen {
    fn of(paths: &[PathBuf]) -> Chosen {
        let mut pidls = Vec::new();
        for path in paths {
            let mut pidl = null_mut();
            let hr = unsafe {
                SHParseDisplayName(
                    wide(path.as_os_str()).as_ptr(),
                    null_mut(),
                    &mut pidl,
                    0,
                    null_mut(),
                )
            };
            assert_eq!(hr, S_OK, "{}", path.display());
            pidls.push(pidl);
        }
        let count = u32::try_from(pidls.len()).unwrap();
        let mut array = null_mut();
        let hr = unsafe { SHCreateShellItemArrayFromIDLists(count, pidls.as_ptr(), &mut array) };
        assert_eq!(hr, S_OK);
        for pidl in pidls {
            unsafe { CoTaskMemFree(pidl) };
        }
        Chosen(array)
    }
}

impl Drop for Chosen {
    fn drop(&mut self) {
        unsafe { (vtbl::<UnknownVtbl>(self.0).release)(self.0) };
    }
}

/// One of the extension's commands.
struct Cmd(Raw);

impl Cmd {
    fn v(&self) -> &CommandVtbl {
        unsafe { vtbl(self.0) }
    }

    fn title(&self) -> String {
        let mut out = null_mut();
        assert_eq!(
            unsafe { (self.v().get_title)(self.0, null_mut(), &mut out) },
            S_OK
        );
        unsafe { take(out) }
    }

    fn icon(&self) -> Option<String> {
        let mut out = null_mut();
        let hr = unsafe { (self.v().get_icon)(self.0, null_mut(), &mut out) };
        (hr == S_OK).then(|| unsafe { take(out) })
    }

    fn shows(&self, chosen: &Chosen) -> bool {
        let mut state = u32::MAX;
        assert_eq!(
            unsafe { (self.v().get_state)(self.0, chosen.0, 1, &mut state) },
            S_OK
        );
        assert!(state == ECS_ENABLED || state == ECS_HIDDEN, "{state}");
        state == ECS_ENABLED
    }

    fn flags(&self) -> u32 {
        let mut flags = u32::MAX;
        assert_eq!(unsafe { (self.v().get_flags)(self.0, &mut flags) }, S_OK);
        flags
    }

    fn invoke(&self, chosen: &Chosen) {
        assert_eq!(
            unsafe { (self.v().invoke)(self.0, chosen.0, null_mut()) },
            S_OK
        );
    }

    /// The items, one by one as Explorer asks for them.
    fn items(&self) -> Vec<Cmd> {
        let mut list = null_mut();
        assert_eq!(
            unsafe { (self.v().enum_sub_commands)(self.0, &mut list) },
            S_OK
        );
        let v: &CommandsVtbl = unsafe { vtbl(list) };
        let mut out = Vec::new();
        loop {
            let mut item = null_mut();
            let mut fetched = 0;
            unsafe { (v.next)(list, 1, &mut item, &mut fetched) };
            if fetched == 0 {
                break;
            }
            out.push(Cmd(item));
        }
        unsafe { (v.release)(list) };
        out
    }
}

impl Drop for Cmd {
    fn drop(&mut self) {
        unsafe { (self.v().release)(self.0) };
    }
}

/// The extension cargo built for these tests, beside their binary in
/// `deps`.
fn built() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let dll = exe.with_file_name("context_menu_windows.dll");
    assert!(dll.exists(), "{} is not built", dll.display());
    dll
}

/// A fresh scratch folder.
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("context-menu-windows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The menu's command, from the extension at `dll` as COM gets it.
fn menu_command(dll: &Path, id: &str) -> Cmd {
    let module = unsafe { LoadLibraryW(wide(dll.as_os_str()).as_ptr()) };
    assert!(!module.is_null(), "{} does not load", dll.display());
    let entry = unsafe { GetProcAddress(module, c"DllGetClassObject".as_ptr().cast()) };
    assert!(!entry.is_null(), "no DllGetClassObject");
    let get_class: unsafe extern "system" fn(*const Guid, *const Guid, *mut Raw) -> Hresult = unsafe {
        std::mem::transmute::<
            Raw,
            unsafe extern "system" fn(*const Guid, *const Guid, *mut Raw) -> Hresult,
        >(entry)
    };

    let mut factory = null_mut();
    let wrong = Guid::parse(&windows::clsid("another-menu"));
    let iid = Guid::parse(IID_ICLASSFACTORY);
    assert_ne!(
        unsafe { get_class(&wrong, &iid, &mut factory) },
        S_OK,
        "another menu's class"
    );
    let clsid = Guid::parse(&windows::clsid(id));
    assert_eq!(unsafe { get_class(&clsid, &iid, &mut factory) }, S_OK);
    let fv: &FactoryVtbl = unsafe { vtbl(factory) };
    let mut command = null_mut();
    let iid = Guid::parse(IID_IEXPLORERCOMMAND);
    assert_eq!(
        unsafe { (fv.create_instance)(factory, null_mut(), &iid, &mut command) },
        S_OK
    );
    unsafe { (fv.release)(factory) };
    Cmd(command)
}

/// A PowerShell script writing its arguments after the first to the file
/// the first names, one per line.
fn recorder(dir: &Path) -> (PathBuf, Vec<String>) {
    let script = dir.join("record.ps1");
    std::fs::write(
        &script,
        "param($out)\n$args | Set-Content -LiteralPath \"$out.tmp\" -Encoding UTF8\n\
         Move-Item -LiteralPath \"$out.tmp\" -Destination $out\n",
    )
    .unwrap();
    let root = std::env::var("SystemRoot").unwrap();
    let powershell = format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe");
    let args = vec![
        "-NoProfile".to_string(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        script.display().to_string(),
        dir.join("ran.txt").display().to_string(),
    ];
    (PathBuf::from(powershell), args)
}

#[test]
fn the_extension_shows_the_menu_in_explorer() {
    let dir = scratch();
    // As the app's package ships it: renamed for its menu.
    let dll = dir.join(windows::native_name("photo-kit"));
    std::fs::copy(built(), &dll).unwrap();
    // The definition, where the extension reads it: this test's data
    // folder, the only one the process has.
    let data_home = dir.join("AppData");
    unsafe { std::env::set_var("LOCALAPPDATA", &data_home) };

    let (program, args) = recorder(&dir);
    let icon = r"C:\Windows\System32\shell32.dll,3";
    let menu = Menu::new("photo-kit", "Photo Kit")
        .submenu(true)
        .item(
            Item::new("pack", "Archives", &program)
                .args(args)
                .extensions(["zip", "tar.gz"]),
        )
        .item(
            Item::new("any", "Anything", &program)
                .icon(icon)
                .folders(true),
        )
        .item(
            Item::new("folder", "One folder", &program)
                .folders_only()
                .count(Count::One),
        );
    let definition = windows::definition_path("photo-kit", &data_home);
    std::fs::create_dir_all(definition.parent().unwrap()).unwrap();
    std::fs::write(&definition, menu.to_definition()).unwrap();

    let files = dir.join("files");
    for name in ["a b.zip", "x.TAR.GZ", "y.gz"] {
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join(name), "").unwrap();
    }
    for name in ["p", "q"] {
        std::fs::create_dir_all(files.join(name)).unwrap();
    }

    assert_eq!(
        unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED) },
        S_OK
    );
    let root = menu_command(&dll, "photo-kit");
    assert_eq!(root.title(), "Photo Kit");
    assert_eq!(root.flags(), ECF_HASSUBCOMMANDS);
    let items = root.items();
    let titles: Vec<String> = items.iter().map(Cmd::title).collect();
    assert_eq!(titles, ["Archives", "Anything", "One folder"]);
    assert_eq!(items[0].icon(), None);
    assert_eq!(items[1].icon().as_deref(), Some(icon));

    // What shows for each choice: the menu, and which items in it.
    let shown = |names: &[&str]| {
        let chosen = Chosen::of(&names.iter().map(|n| files.join(n)).collect::<Vec<_>>());
        let list: Vec<String> = items
            .iter()
            .filter(|i| i.shows(&chosen))
            .map(Cmd::title)
            .collect();
        assert_eq!(root.shows(&chosen), !list.is_empty(), "{names:?}");
        list
    };
    assert_eq!(shown(&["a b.zip"]), ["Archives", "Anything"]);
    assert_eq!(shown(&["x.TAR.GZ"]), ["Archives", "Anything"]);
    assert_eq!(shown(&["y.gz"]), ["Anything"]);
    assert_eq!(shown(&["p"]), ["Anything", "One folder"]);
    // What the classic menu cannot do: one folder alone, and an item only
    // for what it applies to, all of it.
    assert_eq!(shown(&["p", "q"]), ["Anything"]);
    assert_eq!(shown(&["a b.zip", "p"]), ["Anything"]);

    // Chosen: one run, with every path.
    let chosen = [files.join("a b.zip"), files.join("x.TAR.GZ")];
    items[0].invoke(&Chosen::of(&chosen));
    let ran = dir.join("ran.txt");
    let start = Instant::now();
    while !ran.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the program did not run"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let text = std::fs::read_to_string(&ran).unwrap();
    let lines: Vec<&str> = text.trim_start_matches('\u{feff}').lines().collect();
    let expected: Vec<String> = chosen.iter().map(|p| p.display().to_string()).collect();
    assert_eq!(lines, expected);

    // A menu not installed for the user shows nothing.
    std::fs::remove_file(&definition).unwrap();
    assert!(!root.shows(&Chosen::of(&[files.join("a b.zip")])));
}
