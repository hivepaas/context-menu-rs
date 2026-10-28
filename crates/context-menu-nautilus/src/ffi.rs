//! Nautilus's C interface: the three functions a Nautilus extension exports,
//! a GObject type that implements `NautilusMenuProvider`, and the menu items
//! it returns. The declarations follow libnautilus-extension-4's headers
//! (Nautilus 43 and later), which need not be installed: the symbols are
//! Nautilus's own, found when Nautilus loads the library.
//!
//! Every function Nautilus calls catches a panic and does nothing instead,
//! so none crosses into Nautilus.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, OsStr, c_char, c_int, c_uint, c_ulong, c_void};
use std::os::unix::ffi::OsStrExt as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use super::{Action, Entry, Picked};

type GType = usize;
type Gboolean = c_int;

#[repr(C)]
struct GList {
    data: *mut c_void,
    next: *mut GList,
    prev: *mut GList,
}

#[repr(C)]
struct GTypeQuery {
    type_: GType,
    type_name: *const c_char,
    class_size: c_uint,
    instance_size: c_uint,
}

#[repr(C)]
struct GTypeInfo {
    class_size: u16,
    base_init: *const c_void,
    base_finalize: *const c_void,
    class_init: *const c_void,
    class_finalize: *const c_void,
    class_data: *const c_void,
    instance_size: u16,
    n_preallocs: u16,
    instance_init: *const c_void,
    value_table: *const c_void,
}

type InterfaceInit = unsafe extern "C" fn(*mut c_void, *mut c_void);

#[repr(C)]
struct GInterfaceInfo {
    interface_init: Option<InterfaceInit>,
    interface_finalize: Option<InterfaceInit>,
    interface_data: *mut c_void,
}

type GetFileItems = unsafe extern "C" fn(*mut c_void, *mut GList) -> *mut GList;
type GetBackgroundItems = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut GList;

/// `NautilusMenuProviderInterface`, as nautilus-menu-provider.h has it.
#[repr(C)]
struct MenuProviderIface {
    g_type: GType,
    g_instance_type: GType,
    get_file_items: Option<GetFileItems>,
    get_background_items: Option<GetBackgroundItems>,
}

type Destroy = unsafe extern "C" fn(*mut c_void, *mut c_void);

unsafe extern "C" {
    fn g_object_get_type() -> GType;
    fn g_type_query(type_: GType, query: *mut GTypeQuery);
    fn g_type_module_register_type(
        module: *mut c_void,
        parent: GType,
        name: *const c_char,
        info: *const GTypeInfo,
        flags: c_uint,
    ) -> GType;
    fn g_type_module_add_interface(
        module: *mut c_void,
        instance: GType,
        interface: GType,
        info: *const GInterfaceInfo,
    );
    fn g_object_unref(object: *mut c_void);
    fn g_free(memory: *mut c_void);
    fn g_list_append(list: *mut GList, data: *mut c_void) -> *mut GList;
    fn g_signal_connect_data(
        instance: *mut c_void,
        signal: *const c_char,
        handler: *const c_void,
        data: *mut c_void,
        destroy: Option<Destroy>,
        flags: c_int,
    ) -> c_ulong;
    fn g_file_get_path(file: *mut c_void) -> *mut c_char;

    fn nautilus_menu_provider_get_type() -> GType;
    fn nautilus_file_info_get_uri_scheme(file: *mut c_void) -> *mut c_char;
    fn nautilus_file_info_is_directory(file: *mut c_void) -> Gboolean;
    fn nautilus_file_info_get_name(file: *mut c_void) -> *mut c_char;
    fn nautilus_file_info_get_location(file: *mut c_void) -> *mut c_void;
    fn nautilus_menu_item_new(
        name: *const c_char,
        label: *const c_char,
        tip: *const c_char,
        icon: *const c_char,
    ) -> *mut c_void;
    fn nautilus_menu_new() -> *mut c_void;
    fn nautilus_menu_append_item(menu: *mut c_void, item: *mut c_void);
    fn nautilus_menu_item_set_submenu(item: *mut c_void, menu: *mut c_void);
}

/// The menu this library serves, from its file name; none when it was not
/// renamed for one.
static MENU: OnceLock<Option<String>> = OnceLock::new();
/// The type registered, for `nautilus_module_list_types`.
static TYPES: OnceLock<[GType; 1]> = OnceLock::new();

/// Called by Nautilus when it loads the library: registers the menu
/// provider's type, named after the menu, with `module`.
///
/// # Safety
///
/// `module` is the `GTypeModule` Nautilus loads the library with.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nautilus_module_initialize(module: *mut c_void) {
    // SAFETY: as this function's own.
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe { initialize(module) }));
}

/// Called by Nautilus when it unloads the library: nothing to do.
#[unsafe(no_mangle)]
pub extern "C" fn nautilus_module_shutdown() {}

/// Called by Nautilus for the types the library provides: the menu
/// provider's, or none when the library serves no menu.
///
/// # Safety
///
/// Both pointers are writable, as Nautilus passes them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nautilus_module_list_types(
    types: *mut *const GType,
    num_types: *mut c_int,
) {
    let (list, n) = match TYPES.get() {
        Some(t) => (t.as_ptr(), 1),
        None => (null(), 0),
    };
    // SAFETY: Nautilus passes writable pointers; the array lives in a
    // static, so it outlives the call.
    unsafe {
        *types = list;
        *num_types = n;
    }
}

/// The library's own file, as the dynamic loader opened it.
fn own_path() -> Option<PathBuf> {
    // SAFETY: a zeroed Dl_info is valid (null pointers and zeros), and
    // dladdr only writes to it. The address is a function of this library,
    // so dladdr finds it; dli_fname, when set, is a NUL-terminated string
    // the loader keeps while the library is loaded.
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        let address = nautilus_module_initialize as unsafe extern "C" fn(*mut c_void);
        if libc::dladdr(address as *const c_void, &mut info) == 0 || info.dli_fname.is_null() {
            return None;
        }
        let bytes = CStr::from_ptr(info.dli_fname).to_bytes();
        Some(PathBuf::from(OsStr::from_bytes(bytes)))
    }
}

/// # Safety
///
/// As [`nautilus_module_initialize`].
unsafe fn initialize(module: *mut c_void) {
    let path = own_path();
    let id = path.as_deref().and_then(super::menu_id);
    let _ = MENU.set(id.clone());
    let Some(id) = id else {
        let shown = path.map_or_else(|| "this library".into(), |p| p.display().to_string());
        eprintln!(
            "context-menu-nautilus: {shown} is not named libcontext-menu-<menu id>.so, \
             so it shows no menu"
        );
        return;
    };
    let Ok(name) = CString::new(format!("ContextMenuRs-{id}")) else {
        return;
    };
    // SAFETY: GObject's type exists, and g_type_query fills `query`. The
    // type is registered with the module Nautilus gave, as a plain GObject
    // subclass with no fields of its own (GObject's sizes, no init
    // functions); GLib copies `info` and `iface`, and calls
    // `menu_provider_init` with the provider interface's vtable.
    unsafe {
        let object = g_object_get_type();
        let mut query = GTypeQuery {
            type_: 0,
            type_name: null(),
            class_size: 0,
            instance_size: 0,
        };
        g_type_query(object, &mut query);
        let (Ok(class_size), Ok(instance_size)) = (
            u16::try_from(query.class_size),
            u16::try_from(query.instance_size),
        ) else {
            return;
        };
        let info = GTypeInfo {
            class_size,
            base_init: null(),
            base_finalize: null(),
            class_init: null(),
            class_finalize: null(),
            class_data: null(),
            instance_size,
            n_preallocs: 0,
            instance_init: null(),
            value_table: null(),
        };
        let provider = g_type_module_register_type(module, object, name.as_ptr(), &info, 0);
        if provider == 0 {
            return;
        }
        let iface = GInterfaceInfo {
            interface_init: Some(menu_provider_init),
            interface_finalize: None,
            interface_data: null_mut(),
        };
        g_type_module_add_interface(module, provider, nautilus_menu_provider_get_type(), &iface);
        let _ = TYPES.set([provider]);
    }
}

/// Fills the provider interface's vtable.
unsafe extern "C" fn menu_provider_init(iface: *mut c_void, _data: *mut c_void) {
    // SAFETY: GLib calls this with the type's NautilusMenuProviderInterface,
    // whose layout MenuProviderIface follows.
    unsafe {
        let iface = iface.cast::<MenuProviderIface>();
        (*iface).get_file_items = Some(get_file_items);
        (*iface).get_background_items = Some(get_background_items);
    }
}

/// Nautilus asks what to show for the files chosen.
unsafe extern "C" fn get_file_items(_provider: *mut c_void, files: *mut GList) -> *mut GList {
    // SAFETY: Nautilus passes a list of NautilusFileInfo it owns, alive for
    // the call.
    catch_unwind(AssertUnwindSafe(|| unsafe { file_items(files) })).unwrap_or(null_mut())
}

/// Nothing shows for a click on a folder's background.
unsafe extern "C" fn get_background_items(
    _provider: *mut c_void,
    _folder: *mut c_void,
) -> *mut GList {
    null_mut()
}

/// # Safety
///
/// `files` is a list of NautilusFileInfo, alive for the call.
unsafe fn file_items(files: *mut GList) -> *mut GList {
    let Some(Some(id)) = MENU.get() else {
        return null_mut();
    };
    let mut picked = Vec::new();
    let mut node = files;
    while !node.is_null() {
        // SAFETY: `node` is a node of the list, and its data a
        // NautilusFileInfo. The strings and GFile returned are ours, freed
        // here.
        let file = unsafe {
            let file = (*node).data;
            node = (*node).next;
            pick(file)
        };
        // A file with no local path (sftp, smb…): the menu passes paths.
        let Some(file) = file else {
            return null_mut();
        };
        picked.push(file);
    }
    let menu = match super::load(id) {
        Ok(menu) => menu,
        Err(e) => {
            super::warn_once(&e);
            return null_mut();
        }
    };
    let (submenu, entries) = super::entries(&menu, &picked);
    // SAFETY: the items are new objects whose reference this owns; a
    // menu and append_item and set_submenu take their own references, so
    // this drops its own after giving them. The list returned, and the
    // references in it, go to Nautilus.
    unsafe {
        let items: Vec<*mut c_void> = entries
            .into_iter()
            .map(|e| item(e))
            .filter(|i| !i.is_null())
            .collect();
        if items.is_empty() {
            return null_mut();
        }
        match submenu {
            Some(s) => {
                let top = new_item(&s.name, &s.title, s.icon.as_deref());
                if top.is_null() {
                    items.into_iter().for_each(|i| g_object_unref(i));
                    return null_mut();
                }
                let menu = nautilus_menu_new();
                for i in items {
                    nautilus_menu_append_item(menu, i);
                    g_object_unref(i);
                }
                nautilus_menu_item_set_submenu(top, menu);
                g_object_unref(menu);
                g_list_append(null_mut(), top)
            }
            None => items
                .into_iter()
                .fold(null_mut(), |list, i| g_list_append(list, i)),
        }
    }
}

/// What the menu needs of a file: its name, whether it is a folder, and its
/// local path, which a remote file has none of.
///
/// # Safety
///
/// `file` is a live NautilusFileInfo.
unsafe fn pick(file: *mut c_void) -> Option<Picked> {
    // SAFETY: each getter returns a new string, or GFile, that the caller
    // frees; `take` frees the strings, the GFile is unreferenced here.
    unsafe {
        if take(nautilus_file_info_get_uri_scheme(file))?.as_bytes() != b"file" {
            return None;
        }
        let folder = nautilus_file_info_is_directory(file) != 0;
        let name = take(nautilus_file_info_get_name(file))?;
        let location = nautilus_file_info_get_location(file);
        if location.is_null() {
            return None;
        }
        let path = take(g_file_get_path(location));
        g_object_unref(location);
        Some(Picked {
            name: name.to_string_lossy().into_owned(),
            folder,
            path: PathBuf::from(OsStr::from_bytes(path?.as_bytes())),
        })
    }
}

/// A string GLib allocated, copied and freed.
///
/// # Safety
///
/// `s` is null, or a NUL-terminated string the caller owns, from GLib.
unsafe fn take(s: *mut c_char) -> Option<CString> {
    if s.is_null() {
        return None;
    }
    // SAFETY: as this function's own; it is freed once, here.
    unsafe {
        let copy = CStr::from_ptr(s).to_owned();
        g_free(s.cast());
        Some(copy)
    }
}

/// A new menu item, which runs `entry`'s action when chosen; null if a
/// string holds a NUL.
///
/// # Safety
///
/// Called with Nautilus loaded, as everything here.
unsafe fn item(entry: Entry) -> *mut c_void {
    // SAFETY: the item is new; the action is moved to the heap, its
    // pointer given to the signal with `drop_action` to free it when the
    // item goes.
    unsafe {
        let item = new_item(&entry.name, &entry.label, entry.icon.as_deref());
        if item.is_null() {
            return item;
        }
        let data = Box::into_raw(Box::new(entry.action)).cast::<c_void>();
        let handler = activate as unsafe extern "C" fn(*mut c_void, *mut c_void);
        g_signal_connect_data(
            item,
            c"activate".as_ptr(),
            handler as *const c_void,
            data,
            Some(drop_action),
            0,
        );
        item
    }
}

/// # Safety
///
/// As [`item`].
unsafe fn new_item(name: &str, label: &str, icon: Option<&str>) -> *mut c_void {
    let (Ok(name), Ok(label)) = (CString::new(name), CString::new(label)) else {
        return null_mut();
    };
    let icon = icon.and_then(|i| CString::new(i).ok());
    let icon = icon.as_ref().map_or(null(), |i| i.as_ptr());
    // SAFETY: the strings are NUL-terminated, and copied by the item.
    unsafe { nautilus_menu_item_new(name.as_ptr(), label.as_ptr(), null(), icon) }
}

/// An item was chosen: runs its action.
unsafe extern "C" fn activate(_item: *mut c_void, data: *mut c_void) {
    // SAFETY: `data` is the Action `item` gave, alive until `drop_action`.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        super::run(unsafe { &*data.cast::<Action>() })
    }));
}

/// The item went: frees its action.
unsafe extern "C" fn drop_action(data: *mut c_void, _closure: *mut c_void) {
    // SAFETY: `data` is the Box `item` leaked, freed once, here.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(unsafe { Box::from_raw(data.cast::<Action>()) })
    }));
}
