//! Puts a demo menu in the file manager, whose items run this program again
//! with the paths chosen; it shows them in a notification where
//! `notify-send` is, or a message box on Windows, and writes them to
//! `context-menu-demo.log` in the temporary folder.
//!
//! ```text
//! cargo run --example demo -- install
//! cargo run --example demo -- uninstall
//! ```
//!
//! The menu runs the program where cargo built it, so keep it there while
//! trying the menu. On Windows 11, `demo manifest` writes the manifest of a
//! package for the demo, declaring the native extension, which
//! `scripts/windows/demo-package.ps1` registers: see `docs/windows.md`.

use std::io::Write as _;
use std::process::ExitCode;

use context_menu::{Count, Item, Menu};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("install") => install(),
        Some("uninstall") => context_menu::uninstall("context-menu-demo").map(|r| {
            println!("removed {} files", r.removed.len());
        }),
        Some("run") => {
            run(&args[1..]);
            Ok(())
        }
        Some("manifest") => {
            print!("{}", manifest());
            Ok(())
        }
        _ => {
            eprintln!("usage: demo install | uninstall | manifest");
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
    let me = std::env::current_exe().map_err(|e| context_menu::Error::Io {
        path: "the demo itself".into(),
        source: e,
    })?;
    let menu = Menu::new("context-menu-demo", "context-menu demo")
        .submenu(true)
        .icon(icon("applications-utilities", "explorer.exe,0"))
        .item(
            Item::new("archives", "Archives: show their paths", &me)
                .args(["run", "archives"])
                .icon(icon("package-x-generic", r"System32\zipfldr.dll,0"))
                .extensions(["zip", "7z", "rar", "tar.gz", "tar.zst"]),
        )
        .item(
            Item::new("anything", "Anything: show the paths", &me)
                .args(["run", "anything"])
                .icon(icon("text-x-generic", r"System32\shell32.dll,0"))
                .folders(true),
        )
        .item(
            Item::new("one-folder", "One folder: show its path", &me)
                .args(["run", "one-folder"])
                .icon(icon("folder", r"System32\shell32.dll,3"))
                .folders_only()
                .count(Count::One),
        );
    let report = context_menu::install(&menu)?;
    for path in &report.written {
        println!("wrote {}", path.display());
    }
    for note in &report.notes {
        println!("note: {note}");
    }
    Ok(())
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

/// An icon: a theme's name on Linux; on Windows, one of its own, in a file
/// of the Windows folder, with its index.
fn icon(name: &str, windows: &str) -> String {
    if cfg!(windows) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        format!(r"{root}\{windows}")
    } else {
        name.to_string()
    }
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

/// Shows `text` in a notification, or on Windows a message box; with
/// `CONTEXT_MENU_DEMO_QUIET` set, as for tests with no screen, not at all.
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
    } else {
        let _ = std::process::Command::new("notify-send")
            .args(["context-menu demo", text])
            .status();
    }
}
