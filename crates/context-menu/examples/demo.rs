//! Puts a demo menu in the file manager, whose items run this program again
//! with the paths chosen; it shows them in a notification, where
//! `notify-send` is, and writes them to `context-menu-demo.log` in the
//! temporary folder.
//!
//! ```text
//! cargo run --example demo -- install
//! cargo run --example demo -- uninstall
//! ```
//!
//! The menu runs the program where cargo built it, so keep it there while
//! trying the menu.

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
        _ => {
            eprintln!("usage: demo install | uninstall");
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
        .icon("applications-utilities")
        .item(
            Item::new("archives", "Archives: show their paths", &me)
                .args(["run", "archives"])
                .icon("package-x-generic")
                .extensions(["zip", "7z", "rar", "tar.gz", "tar.zst"]),
        )
        .item(
            Item::new("anything", "Anything: show the paths", &me)
                .args(["run", "anything"])
                .folders(true),
        )
        .item(
            Item::new("one-folder", "One folder: show its path", &me)
                .args(["run", "one-folder"])
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
    let _ = std::process::Command::new("notify-send")
        .args(["context-menu demo", &text])
        .status();
}
