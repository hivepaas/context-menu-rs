<#
.SYNOPSIS
  Shows the classic right-click menu Explorer builds for some files, and
  chooses an item of it: the shell's own IContextMenu, as Explorer asks it.

.EXAMPLE
  .\menu.ps1 C:\Users\me\a.zip
  .\menu.ps1 C:\Users\me\a.zip, C:\Users\me\b.zip
  .\menu.ps1 C:\Users\me\a.zip -Choose 'context-menu demo/Archives: show their paths'

.NOTES
  The files must be in one folder, as a selection in Explorer is, and the
  menu is the first one's, as Explorer's is the one right-clicked. Items are
  printed one per line, a submenu's indented under it, and [icon] after
  those with one.
#>
param(
    [Parameter(Mandatory = $true, Position = 0)] [string[]] $Paths,
    # An item's labels from the top, joined by '/', to choose.
    [string] $Choose
)

$ErrorActionPreference = 'Stop'

# Once per session: a type cannot be added twice.
if (-not ('ShellMenu' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class ShellMenu
{
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    static extern int SHParseDisplayName(string name, IntPtr bindCtx, out IntPtr pidl, uint sfgaoIn, out uint sfgaoOut);
    [DllImport("shell32.dll")]
    static extern int SHBindToParent(IntPtr pidl, [In] ref Guid riid, out IntPtr ppv, out IntPtr ppidlLast);
    [DllImport("user32.dll")] static extern IntPtr CreatePopupMenu();
    [DllImport("user32.dll")] static extern int GetMenuItemCount(IntPtr menu);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern bool GetMenuItemInfo(IntPtr menu, uint item, bool byPosition, ref MENUITEMINFO info);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct MENUITEMINFO
    {
        public uint cbSize, fMask, fType, fState, wID;
        public IntPtr hSubMenu, hbmpChecked, hbmpUnchecked, dwItemData, dwTypeData;
        public uint cch;
        public IntPtr hbmpItem;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct CMINVOKECOMMANDINFOEX
    {
        public int cbSize;
        public uint fMask;
        public IntPtr hwnd, lpVerb;
        [MarshalAs(UnmanagedType.LPStr)] public string lpParameters, lpDirectory;
        public int nShow;
        public uint dwHotKey;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.LPStr)] public string lpTitle;
        public IntPtr lpVerbW;
        public string lpParametersW, lpDirectoryW, lpTitleW;
        public int x, y;
    }

    [ComImport, Guid("000214E6-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IShellFolder
    {
        void ParseDisplayName(IntPtr hwnd, IntPtr pbc, [MarshalAs(UnmanagedType.LPWStr)] string name, IntPtr eaten, out IntPtr pidl, IntPtr attrs);
        void EnumObjects(IntPtr hwnd, int flags, out IntPtr list);
        void BindToObject(IntPtr pidl, IntPtr pbc, [In] ref Guid riid, out IntPtr ppv);
        void BindToStorage(IntPtr pidl, IntPtr pbc, [In] ref Guid riid, out IntPtr ppv);
        [PreserveSig] int CompareIDs(IntPtr lParam, IntPtr a, IntPtr b);
        void CreateViewObject(IntPtr hwnd, [In] ref Guid riid, out IntPtr ppv);
        void GetAttributesOf(uint count, [MarshalAs(UnmanagedType.LPArray)] IntPtr[] pidls, ref uint attrs);
        void GetUIObjectOf(IntPtr hwnd, uint count, [MarshalAs(UnmanagedType.LPArray)] IntPtr[] pidls, [In] ref Guid riid, IntPtr reserved, out IntPtr ppv);
    }

    [ComImport, Guid("000214F4-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IContextMenu2
    {
        [PreserveSig] int QueryContextMenu(IntPtr menu, uint index, uint first, uint last, uint flags);
        [PreserveSig] int InvokeCommand(ref CMINVOKECOMMANDINFOEX info);
        [PreserveSig] int GetCommandString(UIntPtr id, uint type, IntPtr reserved, StringBuilder name, uint size);
        [PreserveSig] int HandleMenuMsg(uint msg, IntPtr wParam, IntPtr lParam);
    }

    [ComImport, Guid("BCFCE0A0-EC17-11D0-8D10-00A0C90F2719"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IContextMenu3
    {
        [PreserveSig] int QueryContextMenu(IntPtr menu, uint index, uint first, uint last, uint flags);
        [PreserveSig] int InvokeCommand(ref CMINVOKECOMMANDINFOEX info);
        [PreserveSig] int GetCommandString(UIntPtr id, uint type, IntPtr reserved, StringBuilder name, uint size);
        [PreserveSig] int HandleMenuMsg(uint msg, IntPtr wParam, IntPtr lParam);
        [PreserveSig] int HandleMenuMsg2(uint msg, IntPtr wParam, IntPtr lParam, out IntPtr result);
    }

    const uint First = 1, MIIM_ID = 2, MIIM_SUBMENU = 4, MIIM_CHECKMARKS = 8, MIIM_STRING = 0x40, MIIM_BITMAP = 0x80, MIIM_FTYPE = 0x100;
    const uint MFT_SEPARATOR = 0x800, WM_INITMENUPOPUP = 0x117, CMIC_MASK_UNICODE = 0x4000;

    static IContextMenu2 Menu(string[] paths)
    {
        // Folders hand out IContextMenu; IContextMenu2 is asked of it after.
        Guid folderId = typeof(IShellFolder).GUID, menuId = new Guid("000214E4-0000-0000-C000-000000000046");
        IShellFolder parent = null;
        var children = new IntPtr[paths.Length];
        for (int i = 0; i < paths.Length; i++)
        {
            IntPtr pidl, folder; uint attrs;
            Marshal.ThrowExceptionForHR(SHParseDisplayName(paths[i], IntPtr.Zero, out pidl, 0, out attrs));
            Marshal.ThrowExceptionForHR(SHBindToParent(pidl, ref folderId, out folder, out children[i]));
            if (parent == null) parent = (IShellFolder)Marshal.GetObjectForIUnknown(folder);
        }
        IntPtr ui;
        parent.GetUIObjectOf(IntPtr.Zero, (uint)paths.Length, children, ref menuId, IntPtr.Zero, out ui);
        return (IContextMenu2)Marshal.GetObjectForIUnknown(ui);
    }

    static void Walk(IContextMenu2 cm, IntPtr menu, string prefix, string indent, List<string> lines, Dictionary<string, uint> ids)
    {
        int n = GetMenuItemCount(menu);
        for (uint i = 0; i < n; i++)
        {
            var info = new MENUITEMINFO { cbSize = (uint)Marshal.SizeOf(typeof(MENUITEMINFO)), fMask = MIIM_ID | MIIM_SUBMENU | MIIM_CHECKMARKS | MIIM_STRING | MIIM_BITMAP | MIIM_FTYPE };
            if (!GetMenuItemInfo(menu, i, true, ref info) || (info.fType & MFT_SEPARATOR) != 0) continue;
            var text = new StringBuilder((int)info.cch + 1);
            info.cch++;
            info.dwTypeData = Marshal.StringToHGlobalUni(new string('\0', (int)info.cch));
            GetMenuItemInfo(menu, i, true, ref info);
            string label = Marshal.PtrToStringUni(info.dwTypeData).Replace("&", "");
            Marshal.FreeHGlobal(info.dwTypeData);
            if (label.Length == 0) continue;
            bool icon = info.hbmpItem != IntPtr.Zero || info.hbmpUnchecked != IntPtr.Zero;
            lines.Add(indent + label + (icon ? "  [icon]" : ""));
            string path = prefix + label;
            ids[path] = info.wID;
            if (info.hSubMenu != IntPtr.Zero)
            {
                // A submenu may be filled only as it opens, as Explorer
                // tells the menu with WM_INITMENUPOPUP; until then a
                // submenu of verbs holds an item with no label.
                var cm3 = cm as IContextMenu3;
                IntPtr result;
                if (cm3 != null) cm3.HandleMenuMsg2(WM_INITMENUPOPUP, info.hSubMenu, (IntPtr)i, out result);
                else cm.HandleMenuMsg(WM_INITMENUPOPUP, info.hSubMenu, (IntPtr)i);
                Walk(cm, info.hSubMenu, path + "/", indent + "  ", lines, ids);
            }
        }
    }

    public static string[] Show(string[] paths, string choose)
    {
        var cm = Menu(paths);
        IntPtr menu = CreatePopupMenu();
        Marshal.ThrowExceptionForHR(cm.QueryContextMenu(menu, 0, First, 0x7FFF, 0));
        var lines = new List<string>();
        var ids = new Dictionary<string, uint>();
        Walk(cm, menu, "", "", lines, ids);
        if (!string.IsNullOrEmpty(choose))
        {
            uint id;
            if (!ids.TryGetValue(choose, out id)) throw new ArgumentException("no item " + choose);
            var info = new CMINVOKECOMMANDINFOEX
            {
                cbSize = Marshal.SizeOf(typeof(CMINVOKECOMMANDINFOEX)),
                fMask = CMIC_MASK_UNICODE,
                lpVerb = (IntPtr)(id - First),
                lpVerbW = (IntPtr)(id - First),
                nShow = 1,
            };
            Marshal.ThrowExceptionForHR(cm.InvokeCommand(ref info));
        }
        return lines.ToArray();
    }
}
'@
}

$full = $Paths | ForEach-Object { (Resolve-Path $_).Path }
[ShellMenu]::Show($full, $Choose)
