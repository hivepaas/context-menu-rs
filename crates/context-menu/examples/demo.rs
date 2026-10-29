//! Puts a demo menu in the file manager, whose items run this program again
//! with the paths chosen; it shows them in a notification where
//! `notify-send` is, a message box on Windows or an alert on macOS, and
//! writes them to `context-menu-demo.log` in the temporary folder.
//!
//! ```text
//! cargo run --example demo -- install
//! cargo run --example demo -- uninstall
//! ```
//!
//! The menu runs the program where cargo built it, so keep it there while
//! trying the menu. On Windows 11, `demo manifest` writes the manifest of a
//! package for the demo, declaring the native extension, which
//! `scripts/windows/demo-package.ps1` registers: see `docs/windows.md`. On
//! macOS, `demo bundle <app> <team>` writes the `Info.plist`s of the demo as
//! an app holding the native extension, and their entitlements beside it,
//! which `scripts/macos/demo-app.sh` builds and signs: see `docs/macos.md`.

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

use context_menu::macos::FinderExtension;
use context_menu::{Count, Item, Menu};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("install") => install(),
        Some("uninstall") => context_menu::uninstall("context-menu-demo").map(|r| {
            println!("removed {} files", r.removed.len());
        }),
        Some("status") => menu()
            .and_then(|m| context_menu::is_current(&m))
            .map(|current| {
                println!(
                    "{}",
                    if current {
                        "current"
                    } else {
                        "to install again"
                    }
                );
            }),
        Some("run") => {
            run(&args[1..]);
            Ok(())
        }
        Some("manifest") => {
            print!("{}", manifest());
            Ok(())
        }
        Some("bundle") if args.len() == 3 => bundle(Path::new(&args[1]), &args[2]),
        _ => {
            eprintln!("usage: demo install | uninstall | status | manifest | bundle <app> <team>");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("demo: {e}");
            ExitCode::FAILURE
        }
    }
}

fn install() -> Result<(), context_menu::Error> {
    let report = context_menu::install(&menu()?)?;
    for path in &report.written {
        println!("wrote {}", path.display());
    }
    for note in &report.notes {
        println!("note: {note}");
    }
    Ok(())
}

/// The demo's menu, whose items run this program.
fn menu() -> Result<Menu, context_menu::Error> {
    let me = std::env::current_exe().map_err(|e| context_menu::Error::Io {
        path: "the demo itself".into(),
        source: e,
    })?;
    Ok(Menu::new("context-menu-demo", "context-menu demo")
        .submenu(true)
        .icon(icon(
            "applications-utilities",
            "explorer.exe,0",
            &app_icon(&me),
        ))
        .item(
            Item::new("archives", "Archives: show their paths", &me)
                .args(["run", "archives"])
                .icon(icon(
                    "package-x-generic",
                    r"System32\zipfldr.dll,0",
                    "archivebox",
                ))
                .extensions(["zip", "7z", "rar", "tar.gz", "tar.zst"]),
        )
        .item(
            Item::new("anything", "Anything: show the paths", &me)
                .args(["run", "anything"])
                .icon(icon(
                    "text-x-generic",
                    r"System32\shell32.dll,0",
                    "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/\
                     GenericDocumentIcon.icns",
                ))
                .folders(true),
        )
        .item(
            Item::new("one-folder", "One folder: show its path", &me)
                .args(["run", "one-folder"])
                .icon(icon("folder", r"System32\shell32.dll,3", "NSFolder"))
                .folders_only()
                .count(Count::One),
        ))
}

/// The manifest of a package for the demo, as an app's has it: the app's
/// identity, a program, logos, and the native extension serving the menu,
/// all in the demo's folder, the package's external location. A package
/// for Windows 10 2004 and later; Windows 11 shows the menu in its own.
fn manifest() -> String {
    let extensions: String = context_menu::windows::package_extensions("context-menu-demo")
        .lines()
        .map(|l| format!("        {l}\n"))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
  xmlns:desktop4="http://schemas.microsoft.com/appx/manifest/desktop/windows10/4"
  xmlns:desktop5="http://schemas.microsoft.com/appx/manifest/desktop/windows10/5"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  xmlns:com="http://schemas.microsoft.com/appx/manifest/com/windows10"
  IgnorableNamespaces="uap uap10 desktop4 desktop5 rescap com">
  <Identity Name="ContextMenuRs.Demo" Publisher="CN=context-menu-rs demo" Version="0.1.0.0" ProcessorArchitecture="neutral" />
  <Properties>
    <DisplayName>context-menu demo</DisplayName>
    <PublisherDisplayName>context-menu-rs</PublisherDisplayName>
    <Logo>logo50.png</Logo>
    <uap10:AllowExternalContent>true</uap10:AllowExternalContent>
  </Properties>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
    <rescap:Capability Name="unvirtualizedResources" />
  </Capabilities>
  <Applications>
    <Application Id="Demo" Executable="demo.exe" uap10:TrustLevel="mediumIL" uap10:RuntimeBehavior="win32App">
      <uap:VisualElements AppListEntry="none" DisplayName="context-menu demo" Description="context-menu demo" BackgroundColor="transparent" Square150x150Logo="logo150.png" Square44x44Logo="logo44.png" />
      <Extensions>
{extensions}      </Extensions>
    </Application>
  </Applications>
</Package>
"#
    )
}

/// Writes the `Info.plist`s of the demo as the app `app`, holding the native
/// extension and its helper, signed by the team `team`; and beside the app,
/// the extension's and the helper's entitlements, to sign them with.
fn bundle(app: &Path, team: &str) -> Result<(), context_menu::Error> {
    let finder = FinderExtension::new(
        "context-menu-demo",
        "com.hivepaas.context-menu-demo",
        team,
        "context-menu demo",
    );
    let beside = app.parent().unwrap_or(Path::new("."));
    let files = [
        (app.join("Contents/Info.plist"), app_plist()),
        (
            app.join(context_menu::macos::EXTENSION_PATH)
                .join("Contents/Info.plist"),
            finder.extension_plist(),
        ),
        (
            app.join(context_menu::macos::HELPER_PATH)
                .join("Contents/Info.plist"),
            finder.helper_plist(),
        ),
        (
            beside.join("extension.entitlements"),
            finder.extension_entitlements(),
        ),
        (
            beside.join("helper.entitlements"),
            finder.helper_entitlements(),
        ),
    ];
    for (path, text) in files {
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&path, text))
            .map_err(|source| context_menu::Error::Io {
                path: path.clone(),
                source,
            })?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// The demo app's own `Info.plist`: an app with no Dock icon, whose program
/// is the demo.
fn app_plist() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>com.hivepaas.context-menu-demo</string>
	<key>CFBundleExecutable</key>
	<string>demo</string>
	<key>CFBundleName</key>
	<string>context-menu demo</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleShortVersionString</key>
	<string>0.1</string>
	<key>CFBundleVersion</key>
	<string>1</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>LSUIElement</key>
	<true/>
</dict>
</plist>
"#
    .to_string()
}

/// An icon: a theme's name on Linux; on Windows, one of its own, in a file
/// of the Windows folder, with its index; on macOS, a symbol's name or an
/// image's path.
fn icon(name: &str, windows: &str, macos: &str) -> String {
    if cfg!(windows) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        format!(r"{root}\{windows}")
    } else if cfg!(target_os = "macos") {
        macos.to_string()
    } else {
        name.to_string()
    }
}

/// On macOS, the icon of the app the demo is in, as `scripts/macos/demo-app.sh`
/// puts one there: a file in the app's own bundle. Out of one, a symbol.
fn app_icon(me: &Path) -> String {
    me.ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
        .map(|app| app.join("Contents/Resources/demo.icns"))
        .filter(|icon| icon.is_file())
        .map_or("contextualmenu.and.cursorarrow".into(), |icon| {
            icon.display().to_string()
        })
}

/// What an item runs: `run <item> <paths>...`.
fn run(args: &[String]) {
    let (item, paths) = args
        .split_first()
        .map_or(("?", &[][..]), |(i, p)| (i.as_str(), p));
    let text = format!("{item}: {}", paths.join(", "));
    let log = std::env::temp_dir().join("context-menu-demo.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        let _ = writeln!(f, "{text}");
    }
    show(&text);
}

/// Shows `text` in a notification, on Windows a message box, or on macOS an
/// alert; with `CONTEXT_MENU_DEMO_QUIET` set, as for tests with no screen,
/// not at all.
fn show(text: &str) {
    if std::env::var_os("CONTEXT_MENU_DEMO_QUIET").is_some() {
        return;
    }
    if cfg!(windows) {
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Add-Type -AssemblyName PresentationFramework; \
                 [System.Windows.MessageBox]::Show($env:DEMO_TEXT, 'context-menu demo') | Out-Null",
            ])
            .env("DEMO_TEXT", text)
            .status();
    } else if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "display alert \"context-menu demo\" message (system attribute \"DEMO_TEXT\")",
            ])
            .env("DEMO_TEXT", text)
            .status();
    } else {
        let _ = std::process::Command::new("notify-send")
            .args(["context-menu demo", text])
            .status();
    }
}
