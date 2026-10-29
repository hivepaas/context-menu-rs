<#
.SYNOPSIS
  Puts the demo in Windows 11's own right-click menu: registers a package
  for it, declaring the native extension, as an app's installer would, and
  installs the demo's menu again, which leaves the classic menu to the
  package. -Remove takes the package away, and the classic menu comes back.

.EXAMPLE
  cargo build --example demo
  cargo build -p context-menu-windows
  .\scripts\windows\demo-package.ps1
  .\scripts\windows\demo-package.ps1 -Remove

.NOTES
  Registering a package that is not signed takes Developer Mode (Settings,
  System, For developers), and a desktop session: Windows refuses it over
  SSH. The package's files are the demo's folder, target\<profile>\examples:
  the extension goes there as context-menu-context-menu-demo.dll, with
  logos, and the manifest in target\<profile>\demo-package.
#>
param(
    [string] $Profile = 'debug',
    [switch] $Remove
)

$ErrorActionPreference = 'Stop'

$target = Join-Path (Resolve-Path "$PSScriptRoot\..\..") "target\$Profile"
$examples = Join-Path $target 'examples'
$demo = Join-Path $examples 'demo.exe'

if ($Remove) {
    Get-AppxPackage ContextMenuRs.Demo | Remove-AppxPackage
    & $demo install
    return
}

# The extension, named for the demo's menu, context-menu-demo.
Copy-Item (Join-Path $target 'context_menu_windows.dll') `
    (Join-Path $examples 'context-menu-context-menu-demo.dll') -Force

# Logos, which a package must have: plain squares.
Add-Type -AssemblyName System.Drawing
foreach ($size in 44, 50, 150) {
    $bitmap = New-Object System.Drawing.Bitmap $size, $size
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.Clear([System.Drawing.Color]::FromArgb(59, 130, 246))
    $graphics.Dispose()
    $bitmap.Save((Join-Path $examples "logo$size.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
}

$package = Join-Path $target 'demo-package'
New-Item -ItemType Directory -Force $package | Out-Null
$manifest = Join-Path $package 'AppxManifest.xml'
& $demo manifest | Set-Content -Encoding UTF8 $manifest

Add-AppxPackage -Register $manifest -ExternalLocation $examples
& $demo install
