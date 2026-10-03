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
        assert!(
            offenders.is_empty(),
            "unwrap() в рабочем коде:\n{}",
            offenders.join("\n")
        );
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
        assert!(
            offenders.is_empty(),
            "непроверенный expect():\n{}",
            offenders.join("\n")
        );
        assert!(
            !seen.is_empty(),
            "ни одного expect() — список ALLOWED_EXPECTS устарел"
        );
    }

    /// The exemption above is only sound while this file really is compiled
    /// away outside tests, so the claim is checked rather than assumed.
    #[test]
    fn this_module_is_test_only() {
        let main =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))
                .expect("main.rs");
        assert!(
            main.contains("#[cfg(test)]\nmod guard;"),
            "src/guard.rs обязан быть подключён через #[cfg(test)] mod guard;"
        );
    }
}

/// The frontend is plain JS with no build step and no type checker, so the two
/// contracts it depends on — the i18n tables and the API field names — are
/// checked from here rather than in a browser.
mod frontend {
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Reads a file out of `frontend/`. The JS lives under `frontend/static/`
    /// while the two shells sit at the root, so the prefix is the caller's
    /// business rather than a guess made here.
    fn read(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("frontend")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("не прочитан {}: {}", path.display(), e))
    }

    fn read_static(name: &str) -> String {
        read(&format!("static/{}", name))
    }

    /// Splits one `STRINGS` table out of `app.js`.
    ///
    /// The tables are the only nested block indented by twelve spaces in that
    /// file, which is what makes a textual split safe here. A change to the
    /// formatting of the file breaks this loudly rather than silently
    /// emptying the key list, because a missing `ru` table is an error.
    fn strings_table(source: &str, lang: &str) -> BTreeSet<String> {
        let open = format!("        {}: {{", lang);
        let start = source
            .find(&open)
            .unwrap_or_else(|| panic!("в app.js нет таблицы {}", lang))
            + open.len();
        let rest = &source[start..];
        // The table ends at the closing brace of the `STRINGS` object, which is
        // the only line indented by four spaces and holding `};`.
        let end = rest
            .find("\n    };")
            .unwrap_or_else(|| panic!("таблица {} не закрыта", lang));
        // Everything after the last key is the *next* language's opening line,
        // so the slice stops there.
        let body = match rest[..end].find("\n        en: {") {
            Some(cut) if lang == "ru" => &rest[..cut],
            _ => &rest[..end],
        };
        body.lines()
            .filter_map(|l| {
                // `key: 'value',` on one line. The key is an identifier, so
                // splitting on the first colon cannot run into a colon inside
                // the value, and a line without one is a comment.
                let key = l.trim().split(':').next()?.trim();
                let is_key = !key.is_empty()
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && l.trim_start().starts_with(key);
                is_key.then(|| key.to_string())
            })
            .collect()
    }

    #[test]
    fn the_two_languages_define_exactly_the_same_keys() {
        // A key that exists in one language and not the other renders as the
        // raw key for half the visitors, and nothing in a browser reports it:
        // `I18n.t` falls back to Russian, then to the key itself.
        let app = read_static("app.js");
        let ru = strings_table(&app, "ru");
        let en = strings_table(&app, "en");
        assert!(
            !ru.is_empty() && !en.is_empty(),
            "таблицы пусты: ru={} en={}",
            ru.len(),
            en.len()
        );

        let only_ru: Vec<&String> = ru.difference(&en).collect();
        let only_en: Vec<&String> = en.difference(&ru).collect();
        assert!(only_ru.is_empty(), "ключи есть только в ru: {:?}", only_ru);
        assert!(only_en.is_empty(), "ключи есть только в en: {:?}", only_en);
    }

    #[test]
    fn every_key_the_frontend_asks_for_is_defined_in_both_languages() {
        // `I18n.t('key')` with a literal is the common case, and a typo in one
        // of them is invisible until someone switches language.
        let app = read_static("app.js");
        let ru = strings_table(&app, "ru");
        let en = strings_table(&app, "en");

        let mut used: BTreeSet<String> = BTreeSet::new();
        for name in ["app.js", "catalog.js", "detail.js"] {
            let source = read_static(name);
            for line in source.lines() {
                let code = line.split("//").next().unwrap_or("");
                let mut rest = code;
                while let Some(at) = rest.find("I18n.t(") {
                    rest = &rest[at + "I18n.t(".len()..];
                    let arg = rest.trim_start();
                    if let Some(inner) = arg.strip_prefix('\'') {
                        if let Some(end) = inner.find('\'') {
                            used.insert(inner[..end].to_string());
                        }
                    }
                    rest = arg;
                }
            }
        }
        assert!(
            used.len() > 40,
            "найдено {} ключей — разбор сломался",
            used.len()
        );

        let missing: Vec<&String> = used
            .iter()
            .filter(|k| !ru.contains(*k) || !en.contains(*k))
            .collect();
        assert!(missing.is_empty(), "не переведены: {:?}", missing);
    }

    #[test]
    fn every_data_i18n_attribute_in_the_html_has_a_translation() {
        // The two shells are static, so their keys are enumerable here.
        let ru = strings_table(&read_static("app.js"), "ru");
        let en = strings_table(&read_static("app.js"), "en");
        let mut used: BTreeSet<String> = BTreeSet::new();
        for page in ["index.html", "anime.html"] {
            let html = read(page);
            let mut rest = html.as_str();
            while let Some(at) = rest.find("data-i18n") {
                rest = &rest[at..];
                let Some(open) = rest.find('"') else { break };
                let Some(close) = rest[open + 1..].find('"') else {
                    break;
                };
                let key = &rest[open + 1..open + 1 + close];
                if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !key.is_empty() {
                    used.insert(key.to_string());
                }
                rest = &rest[open + 1 + close..];
            }
        }
        assert!(
            used.len() >= 10,
            "найдено {} ключей разметки — разбор сломался",
            used.len()
        );
        let missing: Vec<&String> = used
            .iter()
            .filter(|k| !ru.contains(*k) || !en.contains(*k))
            .collect();
        assert!(
            missing.is_empty(),
            "не переведены в разметке: {:?}",
            missing
        );
    }

    /// The status values the frontend offers have to be the ones the API
    /// accepts, and both sides spell them out in their own file.
    #[test]
    fn the_watchlist_statuses_the_frontend_offers_are_the_ones_the_api_accepts() {
        // A status the UI offers but the API rejects turns "save" into a 400
        // with no obvious cause, and a status the API accepts but the UI never
        // offers is a bucket nobody can reach.
        // The UI list is `STATUS_LABELS` in app.js, which is the one place the
        // statuses are written down. `detail.js` builds its dropdown from it, so
        // a second copy no longer exists to drift.
        let app = read_static("app.js");
        let start = app
            .find("const STATUS_LABELS = {")
            .expect("STATUS_LABELS в app.js")
            + "const STATUS_LABELS = {".len();
        // Each entry is `name: { ru: '…', en: '…' },` on one line, so a naive
        // split on `}` would stop at the first value. The object ends at the
        // line that closes the declaration, which is the only `};` on its own.
        let end = app[start..]
            .find("\n    };")
            .expect("STATUS_LABELS не закрыта");
        let offered: BTreeSet<String> = app[start..start + end]
            .lines()
            .filter_map(|l| {
                let key = l.trim().split(':').next()?.trim();
                (!key.is_empty()
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && l.trim_start().starts_with(key))
                .then(|| key.to_string())
            })
            .collect();
        // Five, and `paused` among them: the parser has to be reading the table
        // rather than something smaller, or the rest of the test compares two
        // lists it invented.
        assert!(
            offered.contains("paused"),
            "paused не разобран: {:?}",
            offered
        );
        assert_eq!(offered.len(), 5, "разобрано: {:?}", offered);
        assert!(
            read_static("detail.js")
                .contains("const STATUS_OPTIONS = Object.keys(App.statusLabels)"),
            "detail.js должен брать статусы из STATUS_LABELS, а не из своего списка"
        );

        // The catalogue's list chips are the same buckets, and they are the only
        // way a `paused` row is reachable now, so they have to come from the
        // same table too.
        let catalog = read_static("catalog.js");
        assert!(
            catalog.contains("Object.keys(App.statusLabels).map"),
            "catalog.js должен брать корзины из STATUS_LABELS"
        );
        assert!(
            catalog.contains("App.isListBucket(next.in_list)"),
            "catalog.js должен отбрасывать неизвестный in_list из ссылки"
        );

        let api = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api/catalog.rs"),
        )
        .expect("catalog.rs");
        let valid = api
            .lines()
            .find(|l| l.contains("VALID_STATUSES:"))
            .expect("VALID_STATUSES в catalog.rs");
        // The line is `pub const VALID_STATUSES: &[&str] = &[...]`, so the
        // values are the part after the *last* `&[`.
        let accepted: BTreeSet<String> = valid
            .rsplit("&[")
            .next()
            .and_then(|s| s.split(']').next())
            .map(|s| {
                s.split(',')
                    .filter_map(|q| {
                        let q = q.trim().trim_matches('"').trim();
                        (!q.is_empty()).then(|| q.to_string())
                    })
                    .collect()
            })
            .expect("список статусов API");
        assert!(!accepted.is_empty(), "VALID_STATUSES пуст: {}", valid);

        let not_accepted: Vec<&String> =
            offered.iter().filter(|s| !accepted.contains(*s)).collect();
        assert!(
            not_accepted.is_empty(),
            "фронтенд предлагает, API отвергает: {:?}",
            not_accepted
        );

        // The reverse is a bucket the API stores into and no screen can ever
        // show, and a status row that counts towards a tab nobody can open.
        let unreachable: Vec<&String> = accepted.iter().filter(|s| !offered.contains(*s)).collect();
        assert!(
            unreachable.is_empty(),
            "API принимает статусы, которых нет в интерфейсе: {:?}",
            unreachable
        );
    }

    /// The sort chips on the catalogue and the `ORDER BY` whitelist.
    ///
    /// `order_clause` falls back to the default for anything it does not know,
    /// so a sort the UI offers but the API lacks is not an error: the chip
    /// appears to do nothing and the order silently stays the default. A sort
    /// the API has and the UI does not is the reverse, and just as quiet.
    #[test]
    fn every_sort_the_frontend_offers_is_one_the_api_orders_by() {
        let catalog = read_static("catalog.js");
        let start =
            catalog.find("const SORTS = [").expect("SORTS в catalog.js") + "const SORTS = [".len();
        let end = catalog[start..].find("];").expect("SORTS не закрыт");
        let offered: BTreeSet<String> = catalog[start..start + end]
            .lines()
            .filter_map(|l| {
                // The entry is `['value', 'i18n_key'],` — the value is the first
                // quoted string, the second one is a translation key that
                // `I18n.t` is checked against separately.
                let mut parts = l.split('\'').skip(1);
                parts.next().map(String::from)
            })
            .collect();
        assert!(offered.len() >= 10, "разобрано: {:?}", offered);

        let api = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api/catalog.rs"),
        )
        .expect("catalog.rs");
        let accepted: BTreeSet<String> = api
            .lines()
            .filter(|l| l.contains("=> \"ORDER BY"))
            .filter_map(|l| l.split('"').nth(1).map(String::from))
            .collect();
        assert!(
            accepted.contains("title_ru"),
            "список сортировок не разобран"
        );

        // `popularity` is the default arm of the match rather than a listed
        // one, so it is added back by hand — a chip for it is legitimate, and
        // the fallback is what would answer it anyway.
        let unknown: Vec<&String> = offered
            .iter()
            .filter(|s| !accepted.contains(*s) && s.as_str() != "popularity")
            .collect();
        assert!(
            unknown.is_empty(),
            "чип сортировки без ORDER BY в API: {:?}",
            unknown
        );
    }

    /// The catalogue filter keys are the parameter names of `ListQuery`.
    ///
    /// `deny_unknown_fields` is what turned the old `has_ru` typo into a 400
    /// that blanked the entire catalogue page, so a key the API does not know
    /// is the single most expensive kind of drift on this boundary. The check
    /// runs in both directions: an unknown key breaks the page, and a key nobody
    /// sends is a filter that cannot be used.
    #[test]
    fn every_catalogue_filter_key_is_a_parameter_the_api_accepts() {
        let catalog = read_static("catalog.js");
        let start = catalog
            .find("const defaults = () => ({")
            .expect("defaults в catalog.js")
            + "const defaults = () => ({".len();
        let end = catalog[start..].find("});").expect("defaults не закрыт");
        let sent: BTreeSet<String> = catalog[start..start + end]
            .lines()
            .filter_map(|l| {
                let key = l.split(':').next()?.trim();
                (!key.is_empty() && l.contains(':')).then(|| key.to_string())
            })
            .collect();
        assert!(sent.len() >= 10, "разобрано: {:?}", sent);

        let models =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/models.rs"))
                .expect("models.rs");
        let query = models
            .split("pub struct ListQuery {")
            .nth(1)
            .and_then(|s| s.split('}').next())
            .expect("ListQuery в models.rs");
        let accepted: BTreeSet<String> = query
            .lines()
            .filter_map(|l| l.split("pub ").nth(1)?.split(':').next())
            .map(|k| k.trim().to_string())
            .collect();
        assert!(
            accepted.contains("has_russian") && accepted.contains("in_list"),
            "список параметров не разобран: {:?}",
            accepted
        );

        let unknown: Vec<&String> = sent.iter().filter(|k| !accepted.contains(*k)).collect();
        assert!(
            unknown.is_empty(),
            "фронтенд шлёт параметр, которого нет в ListQuery (даст 400 на всю страницу): {:?}",
            unknown
        );
    }
}
