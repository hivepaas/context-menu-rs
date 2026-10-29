//! Explorer's COM interface: the two functions a COM library exports, a
//! class factory, the `IExplorerCommand`s of the menu and of its items, and
//! the `IEnumExplorerCommand` listing the items. The declarations follow the
//! Windows SDK's headers (`unknwn.h`, `shobjidl_core.h`).
//!
//! Every function Explorer calls catches a panic and fails instead, so none
//! crosses into Explorer.

#![allow(unsafe_code)]

use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::process::Stdio;
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use super::{Command, Picked, action, has_items, icon, items, load, menu_id, shows, title};

type Hresult = i32;
type Raw = *mut c_void;

/// A COM GUID, as it is in memory.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

impl Guid {
    /// As `context_menu::windows::clsid` writes it.
    fn text(&self) -> String {
        let d = self.data4;
        format!(
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.data1, self.data2, self.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
        )
    }
}

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Guid {
    Guid {
        data1,
        data2,
        data3,
        data4,
    }
}

const IID_IUNKNOWN: Guid = guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_ICLASSFACTORY: Guid = guid(1, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_IEXPLORERCOMMAND: Guid = guid(
    0xA08C_E4D0,
    0xFA25,
    0x44AB,
    [0xB5, 0x7C, 0xC7, 0xB1, 0xC3, 0x23, 0xE0, 0xB9],
);
const IID_IENUMEXPLORERCOMMAND: Guid = guid(
    0xA888_26F8,
    0x186F,
    0x4987,
    [0xAA, 0xDE, 0xEA, 0x0C, 0xEF, 0x8F, 0xBF, 0xE8],
);

const S_OK: Hresult = 0;
const S_FALSE: Hresult = 1;
const E_NOTIMPL: Hresult = 0x8000_4001_u32 as i32;
const E_NOINTERFACE: Hresult = 0x8000_4002_u32 as i32;
const E_POINTER: Hresult = 0x8000_4003_u32 as i32;
const E_FAIL: Hresult = 0x8000_4005_u32 as i32;
const E_OUTOFMEMORY: Hresult = 0x8007_000E_u32 as i32;
const CLASS_E_NOAGGREGATION: Hresult = 0x8004_0110_u32 as i32;
const CLASS_E_CLASSNOTAVAILABLE: Hresult = 0x8004_0111_u32 as i32;

const ECS_ENABLED: u32 = 0;
const ECS_HIDDEN: u32 = 2;
const ECF_DEFAULT: u32 = 0;
const ECF_HASSUBCOMMANDS: u32 = 1;
const SIGDN_FILESYSPATH: i32 = 0x8005_8000_u32 as i32;
const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 2;
const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 4;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut Raw) -> i32;
    fn GetModuleFileNameW(module: Raw, name: *mut u16, size: u32) -> u32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemAlloc(size: usize) -> Raw;
    fn CoTaskMemFree(memory: Raw);
}

/// The objects alive, and the locks Explorer holds: the library may be
/// unloaded when there are none.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Runs `f`, a call from Explorer, failing it rather than letting a panic
/// cross into Explorer.
fn guard(f: impl FnOnce() -> Hresult) -> Hresult {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(E_FAIL)
}

/// The interface a vtable is of, which `QueryInterface` answers to.
trait Interface: 'static {
    const IID: Guid;
}

/// One of the library's COM objects: its vtable, the references to it, and
/// what it holds. A pointer to it is a pointer to the interface.
#[repr(C)]
struct Object<V: 'static, T> {
    vtbl: &'static V,
    refs: AtomicU32,
    value: T,
}

impl<V: Interface, T> Object<V, T> {
    /// A new object with one reference, the caller's.
    fn create(vtbl: &'static V, value: T) -> Raw {
        LIVE.fetch_add(1, Ordering::AcqRel);
        let object = Box::new(Object {
            vtbl,
            refs: AtomicU32::new(1),
            value,
        });
        Box::into_raw(object).cast()
    }

    /// What the object at `this` holds.
    ///
    /// # Safety
    ///
    /// `this` is one of these objects, alive.
    unsafe fn of<'a>(this: Raw) -> &'a T {
        unsafe { &(*this.cast::<Self>()).value }
    }
}

unsafe extern "system" fn query_interface<V: Interface, T>(
    this: Raw,
    iid: *const Guid,
    out: *mut Raw,
) -> Hresult {
    if out.is_null() || iid.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM's rules: `out` and `iid` point where they should.
    let iid = unsafe {
        *out = null_mut();
        *iid
    };
    if iid != IID_IUNKNOWN && iid != V::IID {
        return E_NOINTERFACE;
    }
    // SAFETY: `this` is the object Explorer called.
    unsafe {
        add_ref::<V, T>(this);
        *out = this;
    }
    S_OK
}

unsafe extern "system" fn add_ref<V: Interface, T>(this: Raw) -> u32 {
    // SAFETY: `this` is the object Explorer called.
    let object = unsafe { &*this.cast::<Object<V, T>>() };
    object.refs.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
}

unsafe extern "system" fn release<V: Interface, T>(this: Raw) -> u32 {
    // SAFETY: `this` is the object Explorer called.
    let object = unsafe { &*this.cast::<Object<V, T>>() };
    let left = object.refs.fetch_sub(1, Ordering::AcqRel).wrapping_sub(1);
    if left == 0 {
        // SAFETY: the last reference is gone: the object is ours to free.
        drop(unsafe { Box::from_raw(this.cast::<Object<V, T>>()) });
        LIVE.fetch_sub(1, Ordering::AcqRel);
    }
    left
}

/// `text` where the caller frees it with `CoTaskMemFree`, as COM hands
/// strings out.
///
/// # Safety
///
/// `out` points where a string pointer goes.
unsafe fn hand_out(text: &str, out: *mut *mut u16) -> Hresult {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: a plain allocation, of the string's size.
    let memory = unsafe { CoTaskMemAlloc(wide.len() * 2) }.cast::<u16>();
    if memory.is_null() {
        return E_OUTOFMEMORY;
    }
    // SAFETY: `memory` has room for `wide`, and `out` is the caller's.
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), memory, wide.len());
        *out = memory;
    }
    S_OK
}

// What the library reads of the shell's items: `IShellItemArray` and
// `IShellItem`, the calls before those it makes kept as placeholders.

#[repr(C)]
struct ShellItemArrayVtbl {
    _unknown: [usize; 3],
    _bind_to_handler: usize,
    _get_property_store: usize,
    _get_property_description_list: usize,
    _get_attributes: usize,
    get_count: unsafe extern "system" fn(Raw, *mut u32) -> Hresult,
    get_item_at: unsafe extern "system" fn(Raw, u32, *mut Raw) -> Hresult,
}

#[repr(C)]
struct ShellItemVtbl {
    _query_interface: usize,
    _add_ref: usize,
    release: unsafe extern "system" fn(Raw) -> u32,
    _bind_to_handler: usize,
    _get_parent: usize,
    get_display_name: unsafe extern "system" fn(Raw, i32, *mut *mut u16) -> Hresult,
}

/// The path of a shell item, if it is a file or folder.
///
/// # Safety
///
/// `item` is an `IShellItem`.
unsafe fn path_of(item: Raw) -> Option<PathBuf> {
    // SAFETY: an interface pointer points at its vtable.
    let vtbl = unsafe { &**item.cast::<*const ShellItemVtbl>() };
    let mut name: *mut u16 = null_mut();
    // SAFETY: `item` is an IShellItem.
    let hr = unsafe { (vtbl.get_display_name)(item, SIGDN_FILESYSPATH, &mut name) };
    if hr < 0 || name.is_null() {
        return None;
    }
    // SAFETY: the shell's string ends in a zero, and is ours to free.
    let path = unsafe {
        let len = (0..).take_while(|&i| *name.add(i) != 0).count();
        let path = OsString::from_wide(std::slice::from_raw_parts(name, len));
        CoTaskMemFree(name.cast());
        path
    };
    Some(path.into())
}

/// The files and folders in Explorer's `items`, an `IShellItemArray`: none
/// when one of them has no path, as a phone's or a library's.
///
/// # Safety
///
/// `items` is null or an `IShellItemArray`.
unsafe fn picked(items: Raw) -> Vec<Picked> {
    if items.is_null() {
        return Vec::new();
    }
    // SAFETY: an interface pointer points at its vtable.
    let vtbl = unsafe { &**items.cast::<*const ShellItemArrayVtbl>() };
    let mut count = 0;
    // SAFETY: `items` is an IShellItemArray.
    if unsafe { (vtbl.get_count)(items, &mut count) } < 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..count {
        let mut item: Raw = null_mut();
        // SAFETY: `items` is an IShellItemArray, and `i` within it.
        if unsafe { (vtbl.get_item_at)(items, i, &mut item) } < 0 || item.is_null() {
            return Vec::new();
        }
        // SAFETY: `item` is an IShellItem, with a reference of ours.
        let path = unsafe {
            let path = path_of(item);
            ((**item.cast::<*const ShellItemVtbl>()).release)(item);
            path
        };
        match path.and_then(Picked::at) {
            Some(p) => out.push(p),
            None => return Vec::new(),
        }
    }
    out
}

/// The menu this library serves, from its file's name.
fn own_id() -> Option<&'static str> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(|| {
        let mut module: Raw = null_mut();
        let here = own_id as *const ();
        // SAFETY: `here` is an address in this library.
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                here.cast(),
                &mut module,
            )
        };
        if found == 0 {
            return None;
        }
        let mut name = vec![0u16; 260];
        loop {
            let size = u32::try_from(name.len()).ok()?;
            // SAFETY: `name` has room for `size` characters.
            let len = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), size) } as usize;
            if len == 0 {
                return None;
            }
            if len < name.len() {
                name.truncate(len);
                break;
            }
            // Cut short: a longer path.
            name.resize(name.len() * 2, 0);
        }
        menu_id(&PathBuf::from(OsString::from_wide(&name)))
    })
    .as_deref()
}

// The class factory.

#[repr(C)]
struct FactoryVtbl {
    query_interface: unsafe extern "system" fn(Raw, *const Guid, *mut Raw) -> Hresult,
    add_ref: unsafe extern "system" fn(Raw) -> u32,
    release: unsafe extern "system" fn(Raw) -> u32,
    create_instance: unsafe extern "system" fn(Raw, Raw, *const Guid, *mut Raw) -> Hresult,
    lock_server: unsafe extern "system" fn(Raw, i32) -> Hresult,
}

impl Interface for FactoryVtbl {
    const IID: Guid = IID_ICLASSFACTORY;
}

/// The factory of the menu `id`'s commands.
struct Factory {
    id: String,
}

static FACTORY: FactoryVtbl = FactoryVtbl {
    query_interface: query_interface::<FactoryVtbl, Factory>,
    add_ref: add_ref::<FactoryVtbl, Factory>,
    release: release::<FactoryVtbl, Factory>,
    create_instance,
    lock_server,
};

unsafe extern "system" fn create_instance(
    this: Raw,
    outer: Raw,
    iid: *const Guid,
    out: *mut Raw,
) -> Hresult {
    guard(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `out` is the caller's.
        unsafe { *out = null_mut() };
        if !outer.is_null() {
            return CLASS_E_NOAGGREGATION;
        }
        // SAFETY: `this` is a factory.
        let id = unsafe { Object::<FactoryVtbl, Factory>::of(this) }
            .id
            .clone();
        let command = Object::create(
            &COMMAND,
            Cmd {
                id,
                command: Command::Menu,
            },
        );
        // SAFETY: `command` is ours, with the one reference given back.
        unsafe {
            let hr = query_interface::<CommandVtbl, Cmd>(command, iid, out);
            release::<CommandVtbl, Cmd>(command);
            hr
        }
    })
}

unsafe extern "system" fn lock_server(_this: Raw, lock: i32) -> Hresult {
    if lock != 0 {
        LIVE.fetch_add(1, Ordering::AcqRel);
    } else {
        // One fewer, but never below none: an unlock with no lock is the
        // caller's mistake.
        let mut n = LIVE.load(Ordering::Acquire);
        while n > 0 {
            match LIVE.compare_exchange_weak(n, n - 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(now) => n = now,
            }
        }
    }
    S_OK
}

// The commands: the menu's own, and its items'.

#[repr(C)]
struct CommandVtbl {
    query_interface: unsafe extern "system" fn(Raw, *const Guid, *mut Raw) -> Hresult,
    add_ref: unsafe extern "system" fn(Raw) -> u32,
    release: unsafe extern "system" fn(Raw) -> u32,
    get_title: unsafe extern "system" fn(Raw, Raw, *mut *mut u16) -> Hresult,
    get_icon: unsafe extern "system" fn(Raw, Raw, *mut *mut u16) -> Hresult,
    get_tool_tip: unsafe extern "system" fn(Raw, Raw, *mut *mut u16) -> Hresult,
    get_canonical_name: unsafe extern "system" fn(Raw, *mut Guid) -> Hresult,
    get_state: unsafe extern "system" fn(Raw, Raw, i32, *mut u32) -> Hresult,
    invoke: unsafe extern "system" fn(Raw, Raw, Raw) -> Hresult,
    get_flags: unsafe extern "system" fn(Raw, *mut u32) -> Hresult,
    enum_sub_commands: unsafe extern "system" fn(Raw, *mut Raw) -> Hresult,
}

impl Interface for CommandVtbl {
    const IID: Guid = IID_IEXPLORERCOMMAND;
}

/// A command of the menu `id`.
struct Cmd {
    id: String,
    command: Command,
}

static COMMAND: CommandVtbl = CommandVtbl {
    query_interface: query_interface::<CommandVtbl, Cmd>,
    add_ref: add_ref::<CommandVtbl, Cmd>,
    release: release::<CommandVtbl, Cmd>,
    get_title,
    get_icon,
    get_tool_tip,
    get_canonical_name,
    get_state,
    invoke,
    get_flags,
    enum_sub_commands,
};

/// The command at `this`.
///
/// # Safety
///
/// `this` is a command.
unsafe fn cmd<'a>(this: Raw) -> &'a Cmd {
    unsafe { Object::<CommandVtbl, Cmd>::of(this) }
}

unsafe extern "system" fn get_title(this: Raw, _items: Raw, out: *mut *mut u16) -> Hresult {
    guard(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `out` is the caller's, `this` a command.
        let c = unsafe {
            *out = null_mut();
            cmd(this)
        };
        match load(&c.id).ok().and_then(|m| title(&m, &c.command)) {
            // SAFETY: `out` is the caller's.
            Some(text) => unsafe { hand_out(&text, out) },
            None => E_FAIL,
        }
    })
}

unsafe extern "system" fn get_icon(this: Raw, _items: Raw, out: *mut *mut u16) -> Hresult {
    guard(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `out` is the caller's, `this` a command.
        let c = unsafe {
            *out = null_mut();
            cmd(this)
        };
        match load(&c.id).ok().and_then(|m| icon(&m, &c.command)) {
            // SAFETY: `out` is the caller's.
            Some(text) => unsafe { hand_out(&text, out) },
            None => E_NOTIMPL,
        }
    })
}

unsafe extern "system" fn get_tool_tip(_this: Raw, _items: Raw, out: *mut *mut u16) -> Hresult {
    if !out.is_null() {
        // SAFETY: `out` is the caller's.
        unsafe { *out = null_mut() };
    }
    E_NOTIMPL
}

unsafe extern "system" fn get_canonical_name(_this: Raw, out: *mut Guid) -> Hresult {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: `out` is the caller's.
    unsafe { *out = guid(0, 0, 0, [0; 8]) };
    S_OK
}

unsafe extern "system" fn get_state(this: Raw, items: Raw, _slow: i32, out: *mut u32) -> Hresult {
    guard(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `this` is a command, `items` Explorer's.
        let (c, picked) = unsafe { (cmd(this), picked(items)) };
        let shown = load(&c.id).is_ok_and(|m| shows(&m, &c.command, &picked));
        // SAFETY: `out` is the caller's.
        unsafe { *out = if shown { ECS_ENABLED } else { ECS_HIDDEN } };
        S_OK
    })
}

unsafe extern "system" fn invoke(this: Raw, items: Raw, _context: Raw) -> Hresult {
    guard(|| {
        // SAFETY: `this` is a command, `items` Explorer's.
        let (c, picked) = unsafe { (cmd(this), picked(items)) };
        let Ok(menu) = load(&c.id) else {
            return E_FAIL;
        };
        let Some(mut run) = action(&menu, &c.command, &picked) else {
            return E_FAIL;
        };
        // Explorer has no console for it; the program's own run on.
        let started = run
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if started.is_ok() { S_OK } else { E_FAIL }
    })
}

unsafe extern "system" fn get_flags(this: Raw, out: *mut u32) -> Hresult {
    guard(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `this` is a command.
        let c = unsafe { cmd(this) };
        let submenu = c.command == Command::Menu && load(&c.id).is_ok_and(|m| has_items(&m));
        // SAFETY: `out` is the caller's.
        unsafe {
            *out = if submenu {
                ECF_HASSUBCOMMANDS
            } else {
                ECF_DEFAULT
            }
        };
        S_OK
    })
}

unsafe extern "system" fn enum_sub_commands(this: Raw, out: *mut Raw) -> Hresult {
    guard(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `out` is the caller's, `this` a command.
        let c = unsafe {
            *out = null_mut();
            cmd(this)
        };
        let menu = match load(&c.id) {
            Ok(m) if c.command == Command::Menu && has_items(&m) => m,
            _ => return E_NOTIMPL,
        };
        let list = Object::create(
            &COMMANDS,
            Commands {
                id: c.id.clone(),
                list: items(&menu),
                next: AtomicUsize::new(0),
            },
        );
        // SAFETY: `list` is ours, with the reference handed out.
        unsafe { *out = list };
        S_OK
    })
}

// The items' commands, one after the other.

#[repr(C)]
struct CommandsVtbl {
    query_interface: unsafe extern "system" fn(Raw, *const Guid, *mut Raw) -> Hresult,
    add_ref: unsafe extern "system" fn(Raw) -> u32,
    release: unsafe extern "system" fn(Raw) -> u32,
    next: unsafe extern "system" fn(Raw, u32, *mut Raw, *mut u32) -> Hresult,
    skip: unsafe extern "system" fn(Raw, u32) -> Hresult,
    reset: unsafe extern "system" fn(Raw) -> Hresult,
    clone: unsafe extern "system" fn(Raw, *mut Raw) -> Hresult,
}

impl Interface for CommandsVtbl {
    const IID: Guid = IID_IENUMEXPLORERCOMMAND;
}

/// The menu `id`'s items' commands, and where the list is.
struct Commands {
    id: String,
    list: Vec<Command>,
    next: AtomicUsize,
}

static COMMANDS: CommandsVtbl = CommandsVtbl {
    query_interface: query_interface::<CommandsVtbl, Commands>,
    add_ref: add_ref::<CommandsVtbl, Commands>,
    release: release::<CommandsVtbl, Commands>,
    next,
    skip,
    reset,
    clone,
};

/// The list at `this`.
///
/// # Safety
///
/// `this` is a list of commands.
unsafe fn commands<'a>(this: Raw) -> &'a Commands {
    unsafe { Object::<CommandsVtbl, Commands>::of(this) }
}

unsafe extern "system" fn next(
    this: Raw,
    wanted: u32,
    out: *mut Raw,
    fetched: *mut u32,
) -> Hresult {
    guard(|| {
        // Only one may be asked for without a count to say how many came.
        if out.is_null() || (fetched.is_null() && wanted != 1) {
            return E_POINTER;
        }
        // SAFETY: `this` is a list.
        let list = unsafe { commands(this) };
        let mut given = 0;
        while given < wanted {
            let at = list.next.load(Ordering::Relaxed);
            let Some(command) = list.list.get(at) else {
                break;
            };
            list.next.store(at + 1, Ordering::Relaxed);
            let item = Object::create(
                &COMMAND,
                Cmd {
                    id: list.id.clone(),
                    command: command.clone(),
                },
            );
            // SAFETY: `out` has room for `wanted`, and the reference is the
            // caller's.
            unsafe { *out.add(given as usize) = item };
            given += 1;
        }
        if !fetched.is_null() {
            // SAFETY: `fetched` is the caller's.
            unsafe { *fetched = given };
        }
        if given == wanted { S_OK } else { S_FALSE }
    })
}

unsafe extern "system" fn skip(this: Raw, count: u32) -> Hresult {
    // SAFETY: `this` is a list.
    let list = unsafe { commands(this) };
    let at = list.next.load(Ordering::Relaxed);
    let to = at.saturating_add(count as usize);
    list.next.store(to.min(list.list.len()), Ordering::Relaxed);
    if to <= list.list.len() { S_OK } else { S_FALSE }
}

unsafe extern "system" fn reset(this: Raw) -> Hresult {
    // SAFETY: `this` is a list.
    unsafe { commands(this) }.next.store(0, Ordering::Relaxed);
    S_OK
}

unsafe extern "system" fn clone(this: Raw, out: *mut Raw) -> Hresult {
    guard(|| {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `this` is a list.
        let list = unsafe { commands(this) };
        let copy = Object::create(
            &COMMANDS,
            Commands {
                id: list.id.clone(),
                list: list.list.clone(),
                next: AtomicUsize::new(list.next.load(Ordering::Relaxed)),
            },
        );
        // SAFETY: `out` is the caller's, with the reference handed out.
        unsafe { *out = copy };
        S_OK
    })
}

// What the library exports.

/// Hands out the factory of the menu's commands, when `clsid` is the class
/// of the menu this library's name says (`context_menu::windows::clsid`).
///
/// # Safety
///
/// COM's: the pointers are valid.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const Guid,
    iid: *const Guid,
    out: *mut Raw,
) -> Hresult {
    guard(|| {
        if clsid.is_null() || iid.is_null() || out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `out` and `clsid` are the caller's.
        let clsid = unsafe {
            *out = null_mut();
            *clsid
        };
        let Some(id) = own_id() else {
            return CLASS_E_CLASSNOTAVAILABLE;
        };
        if clsid.text() != context_menu::windows::clsid(id) {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory = Object::create(&FACTORY, Factory { id: id.to_string() });
        // SAFETY: `factory` is ours, with the one reference given back.
        unsafe {
            let hr = query_interface::<FactoryVtbl, Factory>(factory, iid, out);
            release::<FactoryVtbl, Factory>(factory);
            hr
        }
    })
}

/// Whether the library may be unloaded: when none of its objects is alive,
/// and no lock is held.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> Hresult {
    if LIVE.load(Ordering::Acquire) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
