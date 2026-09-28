//! A test that reads the crate's own source.
//!
//! The rule this project cannot bend is that data arriving from outside —
//! AniList, Kitsu, Shikimori, an HTTP header, a query parameter — is never
//! unwrapped. The rule is easy to state and easy to break, and clippy will not
//! catch a violation: `unwrap` is not a lint, it is a method call.
//!
//! So it is checked mechanically on every `cargo test` instead of by review.
//! Every `.rs` file under `src/` is read, everything from its first
//! `#[cfg(test)]` on is discarded (test code is supposed to unwrap — a test
//! that cannot unwrap cannot assert), line comments are stripped so a comment
//! may talk about the rule, and what is left must be free of the panicking
//! calls listed in [`ALLOWED_EXPECTS`].
//!
//! The module is `#[cfg(test)]` at the declaration site in `main.rs`, so none of
//! this exists in the binary.

use std::path::{Path, PathBuf};

/// The only panicking calls allowed in production code, and why each is sound.
///
/// Matched on the message text, not on a line number, so moving code around
/// does not silently disarm the check.
const ALLOWED_EXPECTS: &[&str] = &[
    // `src/db/pool.rs`, in the `Deref`/`DerefMut` of the pool guard. The
    // `Option<Connection>` is filled by `Pool::take` and only `Drop` takes it
    // out again, and `Drop` cannot run while the guard is borrowed, so `None`
    // is unreachable for a live guard. Making the field a plain `Connection`
    // would need `Option::take` in `Drop` to appease the borrow checker, so
    // the invariant is real and is the reason these two calls are allowed.
    "connection already returned to pool",
];

/// This file is exempt, and only this one.
///
/// The scanner has to name the two patterns it forbids, so its own doc comment
/// contains them. It is exempt rather than rewritten because it holds no
/// production code at all — the `#[cfg(test)]` on its declaration in
/// `main.rs` is what guarantees that, and it is asserted by
/// `this_module_is_test_only`.
fn is_exempt(path: &Path) -> bool {
    path.file_name().map(|n| n == "guard.rs").unwrap_or(false)
}

/// The part of `path` that ships: everything before the first `#[cfg(test)]`,
/// with line comments removed.
fn production_lines(path: &Path) -> Vec<(usize, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        // A comment may name the forbidden calls in order to explain them.
        let code = match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        };
        out.push((n + 1, code.to_string()));
    }
    out
}

/// Every `.rs` file under `src/`, sorted so a failure is reproducible.
fn source_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out.sort();
    out
}

/// Every `needle(...)` call on the line, with its argument text.
///
/// The character after the name decides: `_` means a different method with the
/// same prefix (`unwrap_or` is total and must not be reported as a panic), and
/// anything that is not `(` means the name is not a call at all. Whitespace
/// between the name and the paren is tolerated so a wrapped call is still
/// found.
fn calls_in(line: &str, needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(at) = line[from..].find(needle) {
        let start = from + at + needle.len();
        let rest = line[start..].trim_start();
        if !rest.starts_with('_') {
            if let Some(inner) = rest.strip_prefix('(') {
                let mut depth = 1i32;
                let mut end = None;
                for (i, ch) in inner.char_indices() {
                    match ch {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(i);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(end) = end {
                    found.push(unquote(inner[..end].trim()));
                    from = start + 1 + end;
                    continue;
                }
            }
        }
        from = start;
    }
    found
}

/// `"msg"` and `msg` are the same expectation; compare them as the latter.
fn unquote(s: &str) -> String {
    s.trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relative(path: &Path) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// The inventory this guard is built on, kept as a test of its own: a scan
    /// that silently finds no files would pass every other test here and mean
    /// nothing at all.
    #[test]
    fn the_scan_actually_finds_the_sources() {
        let files = source_files();
        assert!(
            files.len() > 20,
            "найдено {} файлов — обход src/ сломан",
            files.len()
        );
        assert!(files.iter().any(|p| p.ends_with("api/auth.rs")));
        assert!(files.iter().any(|p| p.ends_with("db/pool.rs")));
        for f in &files {
            assert!(
                !production_lines(f).is_empty(),
                "{} читается пустым",
                relative(f)
            );
        }
    }

    /// The headline result of the audit that produced this file: the 433
    /// `unwrap()` calls the queue counted live in test code, and no production
    /// path has one.
    #[test]
    fn production_code_never_unwraps() {
        let mut offenders: Vec<String> = Vec::new();
        for file in source_files().iter().filter(|p| !is_exempt(p)) {
            for (line_no, code) in production_lines(file) {
                // `unwrap_or`, `unwrap_or_default` and `unwrap_or_else` are
                // total functions, not panics, so only the bare call counts.
                for call in calls_in(&code, ".unwrap") {
                    offenders.push(format!("{}:{} {}", relative(file), line_no, call));
                }
            }
        }
        assert!(offenders.is_empty(), "unwrap() в рабочем коде:\n{}", offenders.join("\n"));
    }

    #[test]
    fn the_only_production_expects_are_the_allowlisted_ones() {
        let mut offenders: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for file in source_files().iter().filter(|p| !is_exempt(p)) {
            for (line_no, code) in production_lines(file) {
                for msg in calls_in(&code, ".expect") {
                    if !ALLOWED_EXPECTS.iter().any(|ok| ok == &msg) {
                        offenders.push(format!("{}:{} {}", relative(file), line_no, msg));
                    } else {
                        seen.push(format!("{}:{} {}", relative(file), line_no, msg));
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "непроверенный expect():\n{}", offenders.join("\n"));
        assert!(!seen.is_empty(), "ни одного expect() — список ALLOWED_EXPECTS устарел");
    }

    /// The exemption above is only sound while this file really is compiled
    /// away outside tests, so the claim is checked rather than assumed.
    #[test]
    fn this_module_is_test_only() {
        let main = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))
            .expect("main.rs");
        assert!(
            main.contains("#[cfg(test)]\nmod guard;"),
            "src/guard.rs обязан быть подключён через #[cfg(test)] mod guard;"
        );
    }
}
