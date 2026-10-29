<#
.SYNOPSIS
  Shows the apps Explorer offers in Open With for an extension, and opens
  files with one of them: the shell's own list (SHAssocEnumHandlers), as
  Open With asks it.

.EXAMPLE
  .\open-with.ps1 .zip
  .\open-with.ps1 .zip -Open 'context-menu demo' C:\Users\me\a.zip C:\Users\me\b.zip

.NOTES
  Each app is printed on a line of its own, its name, its program or
  ProgID, and [recommended] for those listed for the extension, which
  Open With shows first. -Open opens the files with the app of that name,
  as Open With does.
#>
param(
    [Parameter(Mandatory = $true, Position = 0)] [string] $Extension,
    # An app's name, as listed, to open the files with.
    [string] $Open,
    [Parameter(Position = 1, ValueFromRemainingArguments = $true)] [string[]] $Paths
)

$ErrorActionPreference = 'Stop'

# Once per session: a type cannot be added twice.
if (-not ('OpenWith' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using ComTypes = System.Runtime.InteropServices.ComTypes;

public static class OpenWith
{
    [ComImport, Guid("F04061AC-1659-4a3f-A954-775AA57FC083"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IAssocHandler
    {
        [PreserveSig] int GetName([MarshalAs(UnmanagedType.LPWStr)] out string name);
        [PreserveSig] int GetUIName([MarshalAs(UnmanagedType.LPWStr)] out string name);
        [PreserveSig] int GetIconLocation([MarshalAs(UnmanagedType.LPWStr)] out string path, out int index);
        [PreserveSig] int IsRecommended();
        [PreserveSig] int MakeDefault([MarshalAs(UnmanagedType.LPWStr)] string description);
        [PreserveSig] int Invoke(ComTypes.IDataObject data);
    }

    [ComImport, Guid("973810ae-9599-4b88-9e4d-6ee98c9552da"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IEnumAssocHandlers
    {
        [PreserveSig] int Next(uint count, [Out, MarshalAs(UnmanagedType.LPArray, ArraySubType = UnmanagedType.Interface, SizeParamIndex = 0)] IAssocHandler[] handlers, out uint fetched);
    }

    // Only the first method, BindToHandler, is called.
    [ComImport, Guid("b63ea76d-1f85-456f-a19c-48159efa858b"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IShellItemArray
    {
        [PreserveSig] int BindToHandler(IntPtr bindCtx, [MarshalAs(UnmanagedType.LPStruct)] Guid handler, [MarshalAs(UnmanagedType.LPStruct)] Guid riid, out IntPtr ppv);
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
    static extern void SHAssocEnumHandlers(string extension, int filter, out IEnumAssocHandlers handlers);
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    static extern int SHParseDisplayName(string name, IntPtr bindCtx, out IntPtr pidl, uint sfgaoIn, out uint sfgaoOut);
    [DllImport("shell32.dll", PreserveSig = false)]
    static extern void SHCreateShellItemArrayFromIDLists(uint count, IntPtr[] pidls, out IShellItemArray items);

    static readonly Guid BHID_DataObject = new Guid("B8C0BD9F-ED24-455c-83E6-D5390C4FE8C4");
    static readonly Guid IID_IDataObject = new Guid("0000010e-0000-0000-C000-000000000046");

    static List<IAssocHandler> Handlers(string extension)
    {
        IEnumAssocHandlers e;
        SHAssocEnumHandlers(extension, 0, out e);
        var all = new List<IAssocHandler>();
        var one = new IAssocHandler[1];
        uint fetched;
        while (e.Next(1, one, out fetched) == 0 && fetched == 1)
            all.Add(one[0]);
        return all;
    }

    /// The apps for the extension, one per line.
    public static List<string> List(string extension)
    {
        var lines = new List<string>();
        foreach (var h in Handlers(extension))
        {
            string ui, name;
            h.GetUIName(out ui);
            h.GetName(out name);
            lines.Add(ui + "  (" + name + ")" + (h.IsRecommended() == 0 ? "  [recommended]" : ""));
        }
        return lines;
    }

    /// Opens the files with the app named `ui`, as Open With does: with
    /// the shell's data object holding them.
    public static void Open(string extension, string ui, string[] paths)
    {
        IAssocHandler h = null;
        foreach (var each in Handlers(extension))
        {
            string name;
            each.GetUIName(out name);
            if (name == ui) { h = each; break; }
        }
        if (h == null) throw new ArgumentException("no app named '" + ui + "' in Open With for " + extension);
        var pidls = new IntPtr[paths.Length];
        for (int i = 0; i < paths.Length; i++)
        {
            uint attrs;
            int hr = SHParseDisplayName(paths[i], IntPtr.Zero, out pidls[i], 0, out attrs);
            if (hr != 0) Marshal.ThrowExceptionForHR(hr);
        }
        IShellItemArray items;
        SHCreateShellItemArrayFromIDLists((uint)paths.Length, pidls, out items);
        IntPtr ptr;
        int r = items.BindToHandler(IntPtr.Zero, BHID_DataObject, IID_IDataObject, out ptr);
        if (r != 0) Marshal.ThrowExceptionForHR(r);
        var data = (ComTypes.IDataObject)Marshal.GetObjectForIUnknown(ptr);
        r = h.Invoke(data);
        if (r != 0) Marshal.ThrowExceptionForHR(r);
    }
}
'@
}

if (-not $Open) {
    [OpenWith]::List($Extension)
    return
}
$full = [string[]] @($Paths | ForEach-Object { (Resolve-Path $_).Path })
[OpenWith]::Open($Extension, $Open, $full)
"opened $($full.Count) with $Open"
