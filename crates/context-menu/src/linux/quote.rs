//! Writing text into each kind of file so that it reads back as it was:
//! desktop entries (Dolphin), key files (Nemo), shell (Nautilus scripts and
//! Nemo's command lines) and Python (the Nautilus extension).

/// A value in a desktop entry or a GLib key file: backslashes and line
/// breaks escaped, and a leading space kept.
pub fn key_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ' ' if i == 0 => out.push_str("\\s"),
            c => out.push(c),
        }
    }
    out
}

/// One argument of a desktop entry's `Exec`, quoted as the specification
/// asks when it holds a reserved character, and `%` doubled. The value
/// still goes through [`key_value`], which doubles backslashes again.
pub fn exec_arg(arg: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let arg = arg.replace('%', "%%");
    if !arg.is_empty() && !arg.contains(RESERVED) {
        return arg;
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// A word for a POSIX shell: in single quotes, a quote as `'\''`.
pub fn shell(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// A Python string literal, in single quotes, with every character that is
/// not plain printable ASCII escaped.
pub fn python(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            ' '..='~' => out.push(c),
            c if (c as u32) < 0x100 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push('\'');
    out
}

/// A list of Python string literals.
pub fn python_list<S: AsRef<str>>(items: impl IntoIterator<Item = S>) -> String {
    let inner: Vec<String> = items.into_iter().map(|s| python(s.as_ref())).collect();
    format!("[{}]", inner.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_values_escape_what_key_files_unescape() {
        assert_eq!(key_value("plain"), "plain");
        assert_eq!(key_value(r"a\b"), r"a\\b");
        assert_eq!(key_value(" lead"), r"\slead");
        assert_eq!(key_value("a b"), "a b");
        assert_eq!(key_value("x\ny"), r"x\ny");
    }

    #[test]
    fn exec_args_quote_reserved_characters() {
        assert_eq!(exec_arg("/usr/bin/packwing"), "/usr/bin/packwing");
        assert_eq!(exec_arg("--level=9"), "--level=9");
        assert_eq!(exec_arg("100%"), "100%%");
        assert_eq!(exec_arg("My App"), "\"My App\"");
        assert_eq!(exec_arg("it's"), "\"it's\"");
        assert_eq!(exec_arg("$HOME"), "\"\\$HOME\"");
        assert_eq!(exec_arg(r"C:\x"), r#""C:\\x""#);
        assert_eq!(exec_arg(""), "\"\"");
        // In the file, the backslash quoting a backslash is doubled again.
        assert_eq!(key_value(&exec_arg(r"a\b")), r#""a\\\\b""#);
    }

    #[test]
    fn shell_words_survive_a_shell() {
        assert_eq!(shell("plain"), "'plain'");
        assert_eq!(shell("it's"), r"'it'\''s'");
        assert_eq!(shell("$(rm -rf ~)"), "'$(rm -rf ~)'");
    }

    #[test]
    fn python_literals_are_plain_ascii() {
        assert_eq!(python("Extract here"), "'Extract here'");
        assert_eq!(python("it's \\"), r"'it\'s \\'");
        assert_eq!(python("Nén…"), r"'N\xe9n\u2026'");
        assert_eq!(python("🐝"), r"'\U0001f41d'");
        assert_eq!(python("\n"), r"'\x0a'");
        assert_eq!(python_list(["a", "b"]), "['a', 'b']");
    }
}
