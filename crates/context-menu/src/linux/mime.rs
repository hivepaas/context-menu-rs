//! Extensions as MIME types, from shared-mime-info's `mime/globs2` in each
//! data folder: Dolphin chooses its items by MIME type alone.

use std::path::PathBuf;

/// The MIME types of files named `*.{ext}` (`ext` in lower case), in the
/// order the databases give them, each once.
pub fn types_of(ext: &str, data_dirs: &[PathBuf]) -> Vec<String> {
    let glob = format!("*.{ext}");
    let mut found = Vec::new();
    for dir in data_dirs {
        // A missing or unreadable database is one fewer to look in.
        let Ok(text) = std::fs::read_to_string(dir.join("mime/globs2")) else {
            continue;
        };
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            // weight:type:glob[:flags]
            let mut parts = line.split(':');
            let (Some(_weight), Some(mime), Some(pattern)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let case_sensitive = parts
                .next()
                .is_some_and(|f| f.split(',').any(|f| f == "cs"));
            let matches = if case_sensitive {
                pattern == glob
            } else {
                pattern.to_lowercase() == glob
            };
            if matches && !found.iter().any(|m| m == mime) {
                found.push(mime.to_string());
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::testing::dirs;

    #[test]
    fn extensions_find_their_types() {
        let d = dirs("mime", false);
        assert_eq!(types_of("zip", &d.data_dirs), ["application/zip"]);
        assert_eq!(
            types_of("tar.gz", &d.data_dirs),
            ["application/x-compressed-tar"]
        );
        assert_eq!(types_of("gz", &d.data_dirs), ["application/gzip"]);
        assert!(types_of("nope", &d.data_dirs).is_empty());
    }

    #[test]
    fn the_real_database_knows_archives() {
        let shared = PathBuf::from("/usr/share");
        if !shared.join("mime/globs2").exists() {
            return;
        }
        assert!(types_of("zip", &[shared]).contains(&"application/zip".to_string()));
    }
}
