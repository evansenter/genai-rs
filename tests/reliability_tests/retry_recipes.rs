//! The retry recipes the docs and examples show: `docs/RELIABILITY.md`'s
//! backon recipe matches `examples/retry_with_backoff.rs`, and every retry
//! or poll loop bounds its attempts, sleeps between them, honors
//! `Retry-After` without overflow, caps its delay and, bounded by a count,
//! adds jitter (D-069).

use crate::source_scan;

// =============================================================================
// The backon recipe in docs/RELIABILITY.md and its runnable example
// =============================================================================

/// The lines of the backon recipe in `source`, trimmed: `MAX_DELAY`,
/// `next_delay`, the backoff's builder chain and the retry's `retry`,
/// `when` and `adjust`. Comments and doctest-hidden lines (`# ...`) are not
/// part of it.
fn backon_recipe(source: &str) -> Vec<&str> {
    let lines: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//") && !line.starts_with('#'))
        .collect();
    let mut recipe = Vec::new();
    for (start, end) in [
        ("const MAX_DELAY", ";"),
        ("fn next_delay(", "}"),
        ("let backoff = ExponentialBuilder", ";"),
    ] {
        let from = lines
            .iter()
            .position(|line| line.starts_with(start))
            .expect("the recipe has each of its parts");
        let to = from
            + lines[from..]
                .iter()
                .position(|line| line.ends_with(end))
                .expect("each part of the recipe ends");
        recipe.extend_from_slice(&lines[from..=to]);
    }
    recipe.extend(lines.iter().filter(|line| {
        [".retry(", ".when(", ".adjust("]
            .iter()
            .any(|call| line.starts_with(call))
    }));
    recipe
}

/// `docs/RELIABILITY.md`'s backon snippet (doctested) and
/// `examples/retry_with_backoff.rs` (built) are the same recipe, jitter
/// included: the snippet once lacked the example's `.with_jitter()`, and
/// nothing compiled either one against the other.
#[test]
fn the_backon_recipe_in_reliability_md_matches_its_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = std::fs::read_to_string(root.join("docs/RELIABILITY.md")).unwrap();
    let example = std::fs::read_to_string(root.join("examples/retry_with_backoff.rs")).unwrap();
    let start = doc
        .find("use backon::")
        .expect("RELIABILITY.md has the backon snippet");
    let end = start + doc[start..].find("```").expect("the snippet ends");
    let snippet = &doc[start..end];

    let recipe = backon_recipe(snippet);

    assert_eq!(recipe, backon_recipe(&example));
    assert!(
        recipe.iter().any(|line| line.starts_with(".with_jitter()")),
        "{recipe:#?}"
    );
    assert!(
        recipe.contains(&".when(GenaiError::is_retryable)"),
        "{recipe:#?}"
    );
}

// =============================================================================
// Every retry or poll loop the docs show
// =============================================================================

/// One fenced code block in a doc: where it starts, and its lines (a doc
/// comment's marker stripped).
struct DocBlock {
    at: String,
    lines: Vec<String>,
}

/// The doc text a line of Rust holds, in any of rustdoc's forms: a `///`
/// or `//!` line, a line of a `/** */` or `/*! */` block (`in_block`
/// carries an open one to the next line), or a one-line
/// `#[doc = "..."]` or `#![doc = "..."]` string, plain or raw, whose `\n`
/// escapes split it into lines. `None` when the line holds none.
fn doc_text(line: &str, in_block: &mut bool) -> Option<Vec<String>> {
    let trimmed = line.trim_start();
    if *in_block {
        let (text, closed) = match trimmed.find("*/") {
            Some(end) => (&trimmed[..end], true),
            None => (trimmed, false),
        };
        *in_block = !closed;
        let text = text.strip_prefix('*').unwrap_or(text);
        return Some(vec![text.strip_prefix(' ').unwrap_or(text).to_string()]);
    }
    if let Some(rest) = ["///", "//!"]
        .iter()
        .find_map(|marker| trimmed.strip_prefix(marker))
        .filter(|rest| !rest.starts_with('/'))
    {
        return Some(vec![rest.strip_prefix(' ').unwrap_or(rest).to_string()]);
    }
    if let Some(rest) = ["/**", "/*!"]
        .iter()
        .find_map(|marker| trimmed.strip_prefix(marker))
        .filter(|rest| !rest.starts_with('*') && !rest.starts_with('/'))
    {
        let (text, closed) = match rest.find("*/") {
            Some(end) => (&rest[..end], true),
            None => (rest, false),
        };
        *in_block = !closed;
        return Some(vec![text.trim().to_string()]);
    }
    let value = ["#[doc", "#![doc"]
        .iter()
        .find_map(|marker| trimmed.strip_prefix(marker))?
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let text = if let Some(raw) = value.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let close = format!("\"{}", "#".repeat(hashes));
        let inner = raw[hashes..].strip_prefix('"')?;
        inner[..inner.find(&close)?].to_string()
    } else {
        let mut text = String::new();
        let mut chars = value.strip_prefix('"')?.chars();
        loop {
            match chars.next()? {
                '"' => break,
                '\\' => match chars.next()? {
                    'n' => text.push('\n'),
                    't' => text.push('\t'),
                    other => text.push(other),
                },
                c => text.push(c),
            }
        }
        text
    };
    Some(text.split('\n').map(str::to_string).collect())
}

/// The fenced code blocks in `text`: a Markdown file's, or, for a `.rs`
/// file, its doc comments' ([`doc_text`]).
fn doc_blocks(path: &str, text: &str) -> Vec<DocBlock> {
    let rust = path.ends_with(".rs");
    let mut blocks = Vec::new();
    let mut open: Option<(String, DocBlock)> = None;
    let mut in_block = false;
    for (n, line) in text.lines().enumerate() {
        let docs = if rust {
            doc_text(line, &mut in_block)
        } else {
            Some(vec![line.to_string()])
        };
        let Some(docs) = docs else {
            // Code ends a doc comment, and any block left open in it.
            open = None;
            continue;
        };
        for doc in docs {
            let fence: String = doc
                .trim_start()
                .chars()
                .take_while(|c| *c == '`' || *c == '~')
                .collect();
            match &mut open {
                Some((opened, _))
                    if fence.len() >= opened.len()
                        && fence.starts_with(&opened[..1])
                        && doc.trim() == fence =>
                {
                    blocks.push(open.take().unwrap().1);
                }
                Some((_, block)) => block.lines.push(doc),
                None if fence.len() >= 3 => {
                    let at = format!("{path}:{}", n + 1);
                    open = Some((
                        fence,
                        DocBlock {
                            at,
                            lines: Vec::new(),
                        },
                    ));
                }
                None => {}
            }
        }
    }
    blocks
}

/// A block's code, one entry per line: comments dropped, each string
/// literal's text blanked (its quotes kept), and doctest-hidden lines
/// (`# ...`) left empty. A string or a block comment may span lines; a
/// char literal's quote (`'"'`) opens no string.
fn code_lines(lines: &[String]) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum In {
        Code,
        Str,
        Raw(usize),
        Comment(usize),
    }
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut state = In::Code;
    let mut out = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if state == In::Code && (trimmed.starts_with("# ") || trimmed == "#") {
            out.push(String::new());
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let at = |k: usize| chars.get(k).copied();
        let mut code = String::new();
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            match state {
                In::Comment(depth) if c == '*' && at(k + 1) == Some('/') => {
                    state = if depth == 1 {
                        In::Code
                    } else {
                        In::Comment(depth - 1)
                    };
                    k += 2;
                }
                In::Comment(depth) if c == '/' && at(k + 1) == Some('*') => {
                    state = In::Comment(depth + 1);
                    k += 2;
                }
                In::Str if c == '\\' => k += 2,
                In::Str if c == '"' => {
                    code.push('"');
                    state = In::Code;
                    k += 1;
                }
                In::Raw(hashes) if c == '"' && (1..=hashes).all(|h| at(k + h) == Some('#')) => {
                    code.push('"');
                    state = In::Code;
                    k += 1 + hashes;
                }
                In::Comment(_) | In::Str | In::Raw(_) => k += 1,
                In::Code => {
                    let starts_word = k == 0 || !ident(chars[k - 1]);
                    let hashes = chars[k + 1..].iter().take_while(|h| **h == '#').count();
                    if c == '/' && at(k + 1) == Some('/') {
                        break;
                    } else if c == '/' && at(k + 1) == Some('*') {
                        state = In::Comment(1);
                        k += 2;
                    } else if c == '"' {
                        code.push('"');
                        state = In::Str;
                        k += 1;
                    } else if c == 'r' && starts_word && at(k + 1 + hashes) == Some('"') {
                        code.push('"');
                        state = In::Raw(hashes);
                        k += 2 + hashes;
                    } else if c == '\'' && at(k + 1) == Some('\\') {
                        // `'\n'`, `'\''`, `'\u{7b}'`: to the closing quote.
                        let close = (k + 3..chars.len()).find(|&j| chars[j] == '\'');
                        code.push_str("' '");
                        k = close.map_or(chars.len(), |j| j + 1);
                    } else if c == '\'' && at(k + 2) == Some('\'') {
                        code.push_str("' '");
                        k += 3;
                    } else {
                        code.push(c);
                        k += 1;
                    }
                }
            }
        }
        out.push(code);
    }
    out
}

/// Where a loop opens on `line`: the byte offset of its `loop`, `while` or
/// `for`.
fn loop_keyword(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("while ") || trimmed.starts_with("for ") {
        return Some(line.len() - trimmed.len());
    }
    // `loop {` alone, after `let x =` or `=>`, or after a label.
    line.match_indices("loop {").map(|(at, _)| at).find(|&at| {
        line[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    })
}

/// The line that closes the loop whose keyword is at byte `at` of
/// `code[start]`: where the braces that open after it, outside
/// parentheses, balance again. `None` when no brace opens it (not Rust)
/// or the block ends first.
fn loop_end(code: &[String], start: usize, at: usize) -> Option<usize> {
    let (mut braces, mut parens) = (0usize, 0usize);
    for (k, line) in code.iter().enumerate().skip(start) {
        let text = if k == start { &line[at..] } else { line };
        for c in text.chars() {
            match c {
                '(' | '[' => parens += 1,
                ')' | ']' => parens = parens.saturating_sub(1),
                '{' if braces > 0 || parens == 0 => braces += 1,
                '}' if braces > 0 => {
                    braces -= 1;
                    if braces == 0 {
                        return Some(k);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// `code[k]` from byte `at` on, with the lines after it joined while the
/// statement goes on (the text so far ends in none of `;`, `,`, `{`, `}`),
/// at most four.
fn statement_from(code: &[String], k: usize, at: usize) -> String {
    let mut text = code[k][at..].trim_end().to_string();
    for next in code.iter().skip(k + 1).take(4) {
        if text.ends_with([';', ',', '{', '}']) {
            break;
        }
        text.push(' ');
        text.push_str(next.trim());
    }
    text
}

/// Whether the `.get(` at byte `at` of `code[k]` is awaited before its
/// statement ends: a status request, not a map lookup.
fn awaited(code: &[String], k: usize, at: usize) -> bool {
    let text = statement_from(code, k, at);
    let end = text.find([';', '{', '}']).unwrap_or(text.len());
    text[..end].contains(".await")
}

/// The identifiers in `text`, method and type names included.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| word.starts_with(|c: char| c.is_alphabetic() || c == '_'))
}

/// Every assignment in `code`, as `(name, right side)`: `let [mut] name[:
/// T] = rhs`, `name = rhs` (after a match arm's `=>` too) and `name op=
/// rhs`, recorded as `name op rhs`. The right side runs to the end of its
/// statement.
fn assignments(code: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (k, line) in code.iter().enumerate() {
        let from = line.rfind("=> ").map_or(0, |at| at + 3);
        let statement = line[from..].trim_start();
        let statement = statement.strip_prefix("let ").map_or(statement, |rest| {
            rest.trim_start_matches("mut ").trim_start()
        });
        let name: String = statement
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.starts_with(|c: char| c.is_alphabetic() || c == '_') {
            continue;
        }
        let rest = statement[name.len()..].trim_start();
        let (op, rhs) = if let Some(rhs) = rest.strip_prefix("= ") {
            ("", rhs)
        } else if let Some((op, rhs)) = ["+=", "-=", "*=", "/=", "<<="]
            .iter()
            .find_map(|op| Some((op.trim_end_matches('='), rest.strip_prefix(op)?)))
        {
            (op, rhs)
        } else if rest.starts_with(':') && !rest.starts_with("::") {
            match rest.split_once(" = ") {
                Some((_, rhs)) => ("", rhs),
                None => continue,
            }
        } else {
            continue;
        };
        let at = line.len() - rhs.len();
        let rhs = statement_from(code, k, at);
        let rhs = rhs.trim_end_matches([';', ',']).trim();
        out.push(if op.is_empty() {
            (name, rhs.to_string())
        } else {
            (name.clone(), format!("{name} {op} {rhs}"))
        });
    }
    out
}

fn ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The bracket in `b` that closes the one at `open`.
fn close_of(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// The bracket in `b` that opens the one at `close`.
fn open_of(b: &[u8], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for k in (0..=close).rev() {
        match b[k] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// The method chain in `b` from `at`, each `.name(..)`, `.field`,
/// `.await` and `?` in turn: where it ends, and the methods it calls.
fn postfix(b: &[u8], mut at: usize) -> (usize, Vec<String>) {
    let mut methods = Vec::new();
    loop {
        let mut k = at;
        while b.get(k) == Some(&b' ') {
            k += 1;
        }
        if b.get(k) == Some(&b'?') {
            at = k + 1;
            continue;
        }
        if b.get(k) != Some(&b'.') || !b.get(k + 1).is_some_and(|&c| ident_byte(c)) {
            return (at, methods);
        }
        let mut name_end = k + 1;
        while b.get(name_end).is_some_and(|&c| ident_byte(c)) {
            name_end += 1;
        }
        at = name_end;
        let mut open = name_end;
        while b.get(open) == Some(&b' ') {
            open += 1;
        }
        if b.get(open) == Some(&b'(') {
            methods.push(String::from_utf8_lossy(&b[k + 1..name_end]).into_owned());
            match close_of(b, open) {
                Some(close) => at = close + 1,
                None => return (b.len(), methods),
            }
        }
    }
}

/// Where the receiver of the method call whose `.` is at `dot` starts,
/// walked back over names, paths, calls, indexing, `?` and parentheses.
fn receiver_start(b: &[u8], dot: usize) -> usize {
    let mut at = dot;
    let mut after_dot = true;
    loop {
        let mut k = at;
        if after_dot {
            while k > 0 && b[k - 1] == b' ' {
                k -= 1;
            }
        }
        after_dot = false;
        match k.checked_sub(1).map(|p| b[p]) {
            Some(b'?') => at = k - 1,
            Some(b')' | b']') => match open_of(b, k - 1) {
                Some(open) => at = open,
                None => return k,
            },
            Some(c) if ident_byte(c) => {
                let mut name = k;
                while name > 0 && ident_byte(b[name - 1]) {
                    name -= 1;
                }
                at = name;
                if b[..name].ends_with(b"::") {
                    at = name - 2;
                } else if b[..name].ends_with(b".") && !b[..name].ends_with(b"..") {
                    at = name - 1;
                    after_dot = true;
                } else {
                    return at;
                }
            }
            _ => return at,
        }
    }
}

/// Whether `+`, `-` or `*` (or `+=`, `-=`, `*=`) applies to the operand at
/// `b[start..end]`: each panics on overflow, for an `Instant` and a
/// `Duration` alike.
fn arithmetic_on(b: &[u8], start: usize, end: usize) -> bool {
    let mut k = start;
    while k > 0 && b[k - 1] == b' ' {
        k -= 1;
    }
    let before = &b[..k];
    let assigns = [b"+=", b"-=", b"*="].iter().any(|op| before.ends_with(*op));
    let binary = before.ends_with(b"+") || before.ends_with(b"-") || before.ends_with(b"*");
    // A `-` or `*` with no operand before it is a negation or a deref.
    let operand_before = before[..before.len().saturating_sub(1)]
        .iter()
        .rev()
        .find(|&&c| c != b' ')
        .is_some_and(|&c| ident_byte(c) || matches!(c, b')' | b']' | b'?'));
    let mut k = end;
    while b.get(k) == Some(&b' ') {
        k += 1;
    }
    let after = match b.get(k) {
        Some(b'+' | b'*') => true,
        Some(b'-') => b.get(k + 1) != Some(&b'>'),
        _ => false,
    };
    assigns || (binary && operand_before) || after
}

/// The operands in `text` that hold an uncapped `Retry-After`: each
/// `retry_after()` call, and each name in `held`, with its receiver and
/// its method chain, unless that chain calls `.min(`. As byte ranges.
fn uncapped_operands(text: &str, held: &[String]) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut found = Vec::new();
    for (at, _) in text.match_indices("retry_after") {
        let call = at + "retry_after".len();
        if at == 0 || b[at - 1] != b'.' || b.get(call) != Some(&b'(') {
            continue;
        }
        let Some(close) = close_of(b, call) else {
            continue;
        };
        let (end, methods) = postfix(b, close + 1);
        if !methods.iter().any(|m| m == "min") {
            found.push((receiver_start(b, at - 1), end));
        }
    }
    for name in held {
        for (at, _) in text.match_indices(name.as_str()) {
            let after = at + name.len();
            let whole = (at == 0 || !(ident_byte(b[at - 1]) || b[at - 1] == b'.'))
                && !b
                    .get(after)
                    .is_some_and(|&c| ident_byte(c) || c == b'(' || c == b':');
            if !whole {
                continue;
            }
            let (end, methods) = postfix(b, after);
            if !methods.iter().any(|m| m == "min") {
                found.push((at, end));
            }
        }
    }
    found
}

/// The parameters of the closures along each `retry_after()` call's
/// method chain in `text` (`|asked| asked.max(delay)`): each holds the
/// server's `Retry-After`.
fn retry_after_closure_params(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut params = Vec::new();
    for (at, _) in text.match_indices("retry_after(") {
        let Some(close) = close_of(b, at + "retry_after".len()) else {
            continue;
        };
        let (end, _) = postfix(b, close + 1);
        let mut k = close + 1;
        while k < end {
            if b[k] != b'|' {
                k += 1;
                continue;
            }
            let Some(bar) = (k + 1..end).find(|&j| b[j] == b'|') else {
                break;
            };
            for param in String::from_utf8_lossy(&b[k + 1..bar]).split(',') {
                let param = param.trim().trim_start_matches('&');
                let param = param.strip_prefix("mut ").unwrap_or(param);
                let name: String = param
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() && !params.contains(&name) {
                    params.push(name);
                }
            }
            k = bar + 1;
        }
    }
    params
}

/// The names that hold a value built from an uncapped `retry_after()`
/// (a server's `Retry-After` can be as large as `u64::MAX` seconds): the
/// parameters of the closures along its method chain, and each name
/// assigned an operand that holds it with no `.min(` on its own chain.
fn uncapped_retry_after(text: &str, assigned: &[(String, String)]) -> Vec<String> {
    let mut held = retry_after_closure_params(text);
    loop {
        let before = held.len();
        for (name, rhs) in assigned {
            if !held.contains(name) && !uncapped_operands(rhs, &held).is_empty() {
                held.push(name.clone());
            }
        }
        if held.len() == before {
            return held;
        }
    }
}

/// Whether `op` is a binary operator somewhere in `text`: after an operand,
/// not a deref, a negation or `->`.
fn binary(text: &str, op: u8) -> bool {
    let b = text.as_bytes();
    (0..b.len()).any(|k| {
        b[k] == op
            && b.get(k + 1) != Some(&b'>')
            && b[..k]
                .iter()
                .rev()
                .find(|&&c| c != b' ')
                .is_some_and(|&c| ident_byte(c) || matches!(c, b')' | b']' | b'?'))
    })
}

/// Whether `expr` as a whole is capped: one term whose method chain calls
/// `.min(`, or a call to a `min` function.
fn capped_at_top(expr: &str) -> bool {
    let b = expr.trim().as_bytes();
    let mut k = 0;
    let mut called = false;
    if b.first() == Some(&b'(') {
        let Some(close) = close_of(b, 0) else {
            return false;
        };
        k = close + 1;
    } else {
        loop {
            let name = k;
            while b.get(k).is_some_and(|&c| ident_byte(c)) {
                k += 1;
            }
            if k == name {
                return false;
            }
            if b[k..].starts_with(b"::") {
                k += 2;
                continue;
            }
            if b.get(k) == Some(&b'(') {
                called = &b[name..k] == b"min";
                let Some(close) = close_of(b, k) else {
                    return false;
                };
                k = close + 1;
            }
            break;
        }
    }
    let (end, methods) = postfix(b, k);
    end == b.len() && (called || methods.iter().any(|m| m == "min"))
}

/// Whether `expr`, the time a loop sleeps (or a value it is built from,
/// `of` naming the variable it is assigned to), has a ceiling: a `.min(`
/// over it, or over each value it is built from. A delay that grows has
/// none: one built with `*`, `<<`, `.pow(` or `_mul(`, or from itself with
/// `+` or an `add(`. Nor has the server's `retry_after()`, unless the loop
/// compares the variable that holds it with the time left (`compared`,
/// the names it compares; `by_time`, whether this value reaches one).
fn capped(
    expr: &str,
    of: Option<&str>,
    assigned: &[(String, String)],
    compared: &[&str],
    by_time: bool,
    seen: &mut Vec<String>,
) -> bool {
    if capped_at_top(expr) {
        return true;
    }
    let grows = binary(expr, b'*')
        || ["<<", ".pow(", "_mul("]
            .iter()
            .any(|growth| expr.contains(growth))
        || of.is_some_and(|name| {
            identifiers(expr).any(|id| id == name) && (binary(expr, b'+') || expr.contains("add("))
        });
    if grows || (!by_time && !uncapped_operands(expr, &[]).is_empty()) {
        return false;
    }
    identifiers(expr).all(|name| {
        if seen.iter().any(|s| s == name) {
            return true;
        }
        seen.push(name.to_string());
        let by_time = by_time || compared.contains(&name);
        assigned
            .iter()
            .filter(|(n, _)| n == name)
            .all(|(_, rhs)| capped(rhs, Some(name), assigned, compared, by_time, seen))
    })
}

/// Whether a random value reaches `expr`, the time a loop sleeps: a name
/// in it, or in a value it is built from, that reads as random (`rand`,
/// `random`, `RandomState`, `fastrand`, an `rng`) or as jitter.
fn jittered(expr: &str, assigned: &[(String, String)], seen: &mut Vec<String>) -> bool {
    identifiers(expr).any(|name| {
        let lower = name.to_lowercase();
        if lower.contains("jitter")
            || lower.starts_with("rand")
            || lower.contains("_rand")
            || lower.contains("fastrand")
            || lower.ends_with("rng")
        {
            return true;
        }
        if seen.iter().any(|s| s == name) {
            return false;
        }
        seen.push(name.to_string());
        assigned
            .iter()
            .filter(|(n, _)| n == name)
            .any(|(_, rhs)| jittered(rhs, assigned, seen))
    })
}

/// The argument of the call whose `(` is just before `text`: `text` up to
/// the `)` that closes it.
fn argument(text: &str) -> &str {
    let mut depth = 0usize;
    for (k, c) in text.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => return &text[..k],
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    text
}

const UNBOUNDED: &str = "does not bound its attempts by a count or a deadline";
const NO_SLEEP: &str = "does not sleep between attempts";
const DEAF: &str = "ignores the server's retry_after()";
const GIVES_UP: &str = "ends on the first transient error: it sends a request with `?`, \
     or never checks is_retryable()";
const STALLS: &str = "does not bound each request by the time left (timeout(left, ..))";
const UNCAPPED: &str = "sleeps with no ceiling: growth needs a .min(), and a retry_after() a \
     .min() or a comparison of the wait with the time left";
const OVERFLOWS: &str = "applies +, - or * to an uncapped retry_after(), which panics on a \
     Retry-After too large";
const NO_JITTER: &str = "has no jitter, so clients that failed together retry together";

/// One retry or poll loop in a block: its first line (and that line's
/// index in the block), and what it lacks.
#[derive(Debug)]
struct RetryLoop {
    line: usize,
    head: String,
    problems: Vec<&'static str>,
}

/// The retry and poll loops in `block`, and what each lacks.
///
/// A retry or poll loop is one that calls `is_retryable()`, `sleep(` or
/// an awaited `.get(` (a status request) itself, not in a loop nested in
/// it; a `for` over a collection counts only through `is_retryable()`.
/// Each must:
///
/// - bound its attempts in the loop itself: an attempt count compared with
///   a maximum and incremented, or a comparison with a deadline or an
///   `elapsed()` (a deadline only declared above the loop, or named in a
///   string, is none);
/// - sleep between attempts;
/// - when it sends requests, read the server's `retry_after()`, and retry
///   what `is_retryable()` says to rather than end on the first transient
///   error, as a `?` on an awaited request does;
/// - when a deadline bounds a poll, bound each request by the time left
///   (`timeout(`);
/// - cap the delay it sleeps: growth reaches the sleep through a `.min(`,
///   and so does a `retry_after()`, unless the loop compares the variable
///   that holds it with the time left (a line that names the variable and
///   a deadline, an `elapsed()` or a value built from one, and compares);
/// - apply no `+`, `-` or `*` to an uncapped `retry_after()`, inline, in a
///   variable or in a closure along its chain;
/// - when a count bounds it, add a random value that reaches the sleep.
///
/// D-069 records these rules and their limits.
///
/// Comments, string literals and doctest-hidden lines (`# ...`) don't
/// count.
fn retry_loops(block: &DocBlock) -> Vec<RetryLoop> {
    #[derive(Clone, Copy, Default)]
    struct Calls {
        retries: bool,
        sleeps: bool,
        polls: bool,
    }
    let code = code_lines(&block.lines);
    let loops: Vec<(usize, usize, usize)> = code
        .iter()
        .enumerate()
        .filter_map(|(k, line)| {
            let at = loop_keyword(line)?;
            Some((k, at, loop_end(&code, k, at)?))
        })
        .collect();
    let mut calls = vec![Calls::default(); loops.len()];
    for (k, line) in code.iter().enumerate() {
        let found = Calls {
            retries: line.contains("is_retryable"),
            sleeps: line.contains("sleep("),
            polls: line
                .match_indices(".get(")
                .any(|(at, _)| awaited(&code, k, at)),
        };
        let innermost = loops
            .iter()
            .enumerate()
            .filter(|(_, (start, _, end))| (*start..=*end).contains(&k))
            .max_by_key(|(_, (start, ..))| *start);
        if let Some((n, _)) = innermost {
            calls[n].retries |= found.retries;
            calls[n].sleeps |= found.sleeps;
            calls[n].polls |= found.polls;
        }
    }
    let assigned = assignments(&code);
    let held = uncapped_retry_after(&code.join(" "), &assigned);
    // The names that hold a deadline or the time left.
    let times: Vec<&str> = assigned
        .iter()
        .filter(|(_, rhs)| rhs.contains("deadline") || rhs.contains("elapsed()"))
        .map(|(name, _)| name.as_str())
        .collect();
    loops
        .iter()
        .zip(calls)
        .filter_map(|(&(start, at, end), calls)| {
            let head = code[start][at..].trim();
            let ranged = head.starts_with("for ") && head.contains("..");
            let over_items = head.starts_with("for ") && !ranged;
            if !(calls.retries || !over_items && (calls.sleeps || calls.polls)) {
                return None;
            }
            let body = &code[start..=end];
            let text = body.join(" ");
            let has = |text_: &str| text.contains(text_);
            let compares = |line: &str| {
                [">=", "<=", " > ", " < "]
                    .iter()
                    .any(|op| line.contains(op))
            };
            let counted = ranged
                || (body
                    .iter()
                    .any(|line| line.contains("attempt") && line.contains(" < "))
                    && body
                        .iter()
                        .any(|line| line.contains("attempt") && line.contains("+= ")));
            let timed = body.iter().any(|line| {
                (line.contains("deadline") || line.contains("elapsed()")) && compares(line)
            });
            // What the loop compares with the time left.
            let compared: Vec<&str> = body
                .iter()
                .filter(|line| {
                    compares(line)
                        && (line.contains("deadline")
                            || line.contains("elapsed()")
                            || identifiers(line).any(|id| times.contains(&id)))
                })
                .flat_map(|line| identifiers(line))
                .collect();
            let requests = has("is_retryable") || calls.polls;
            let sleeps: Vec<String> = body
                .iter()
                .enumerate()
                .flat_map(|(k, line)| {
                    line.match_indices("sleep(").map(move |(at, _)| {
                        argument(&statement_from(body, k, at + "sleep(".len())).to_string()
                    })
                })
                .collect();
            let mut problems = Vec::new();
            if !(counted || timed) {
                problems.push(UNBOUNDED);
            }
            if sleeps.is_empty() {
                problems.push(NO_SLEEP);
            }
            if requests && !has("retry_after()") {
                problems.push(DEAF);
            }
            if (calls.polls && !has("is_retryable")) || (requests && has(".await?")) {
                problems.push(GIVES_UP);
            }
            if calls.polls && timed && !has("timeout(") {
                problems.push(STALLS);
            }
            if !sleeps
                .iter()
                .all(|wait| capped(wait, None, &assigned, &compared, false, &mut Vec::new()))
            {
                problems.push(UNCAPPED);
            }
            let bytes = text.as_bytes();
            if uncapped_operands(&text, &held)
                .into_iter()
                .any(|(from, to)| arithmetic_on(bytes, from, to))
            {
                problems.push(OVERFLOWS);
            }
            if counted
                && (sleeps.is_empty()
                    || !sleeps
                        .iter()
                        .all(|wait| jittered(wait, &assigned, &mut Vec::new())))
            {
                problems.push(NO_JITTER);
            }
            Some(RetryLoop {
                line: start,
                head: block.lines[start].trim().to_string(),
                problems,
            })
        })
        .collect()
}

/// Every Markdown file at the top level, and every Markdown and `.rs` file
/// under `docs/`, `examples/` and the library crates' sources:
/// repository-relative path and text.
fn doc_sources() -> Vec<(String, String)> {
    let root = source_scan::root();
    let mut paths = source_scan::files_with_extensions(&root, &["md"], false);
    for dir in ["docs", "examples", "src", "genai-rs-macros/src"] {
        paths.extend(source_scan::files_with_extensions(
            &root.join(dir),
            &["md", "rs"],
            true,
        ));
    }
    let mut found: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            (
                source_scan::repo_relative(path),
                std::fs::read_to_string(path).unwrap(),
            )
        })
        .collect();
    found.sort();
    found
}

/// The code blocks of one source: an example's whole file, which is code;
/// otherwise its fenced blocks ([`doc_blocks`]).
fn scanned_blocks(path: &str, text: &str) -> Vec<DocBlock> {
    if path.starts_with("examples/") && path.ends_with(".rs") {
        vec![DocBlock {
            at: format!("{path}:1"),
            lines: text.lines().map(str::to_string).collect(),
        }]
    } else {
        doc_blocks(path, text)
    }
}

/// A retry or poll loop shown in the docs or an example bounds its
/// attempts, sleeps between them, honors the server's `Retry-After`
/// without overflow, caps its delay and, bounded by a count, adds jitter
/// ([`retry_loops`]), as `docs/RELIABILITY.md`'s recipe does:
/// `InteractionRequest`'s rustdoc loop once retried at full speed with no
/// bound, `Client::execute`'s ignored `retry_after`, the background poll in
/// `AGENTS_AND_BACKGROUND.md` ended on the first transient error, and
/// `examples/deep_research.rs`, the poll's runnable form, did after the
/// doc was fixed. The poll bounds each status request by the time left,
/// too.
#[test]
fn every_retry_loop_in_the_docs_is_bounded_and_waits() {
    let mut loops = Vec::new();
    let mut problems = Vec::new();
    for (path, text) in doc_sources() {
        if path == "CHANGELOG.md" {
            continue;
        }
        for block in scanned_blocks(&path, &text) {
            let found = retry_loops(&block);
            for each in &found {
                for lack in &each.problems {
                    problems.push(format!(
                        "{} (its line {}): the loop `{}` {lack}",
                        block.at,
                        each.line + 1,
                        each.head
                    ));
                }
            }
            if !found.is_empty() {
                loops.push(block);
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    let at: Vec<&str> = loops.iter().map(|block| block.at.as_str()).collect();
    for expected in [
        "src/request/mod.rs:",
        "docs/AGENTS_AND_BACKGROUND.md:",
        "examples/deep_research.rs:",
    ] {
        assert!(
            at.iter().any(|at| at.starts_with(expected)),
            "no retry loop found in {expected} {at:?}"
        );
    }
    for source in [
        "docs/AGENTS_AND_BACKGROUND.md:",
        "examples/deep_research.rs:",
    ] {
        let poll = loops
            .iter()
            .find(|block| block.at.starts_with(source))
            .unwrap();
        let has = |text: &str| poll.lines.iter().any(|line| line.trim() == text);
        assert!(
            has("let left = deadline.saturating_duration_since(Instant::now());")
                && poll
                    .lines
                    .iter()
                    .any(|line| line.contains("timeout(left, client.interactions().get(id))")),
            "{source} the poll bounds each status request by the time left:\n{}",
            poll.lines.join("\n")
        );
    }
}

fn block(text: &str) -> DocBlock {
    DocBlock {
        at: "x".into(),
        lines: text.lines().map(str::to_string).collect(),
    }
}

/// The problems of the one retry or poll loop in `text`.
fn problems(text: &str) -> Vec<&'static str> {
    let found = retry_loops(&block(text));
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap().problems
}

/// The check flags each defect the old recipes had, and passes a loop
/// that has none.
#[test]
fn the_retry_loop_check_finds_each_defect() {
    let unbounded = "let response = loop {\n    match client.execute(request.clone()).await {\n        Ok(r) => break r,\n        Err(e) if e.is_retryable() => continue,\n        Err(e) => return Err(e.into()),\n    }\n};";
    assert_eq!(problems(unbounded), [UNBOUNDED, NO_SLEEP, DEAF]);
    let deaf = "let mut attempts = 0;\nlet response = loop {\n    match client.execute(request.clone()).await {\n        Ok(r) => break r,\n        Err(e) if e.is_retryable() && attempts < 3 => {\n            attempts += 1;\n            tokio::time::sleep(Duration::from_millis(100 * 2u64.pow(attempts))).await;\n        }\n        Err(e) => return Err(e.into()),\n    }\n};";
    assert_eq!(problems(deaf), [DEAF, UNCAPPED, NO_JITTER]);
    // A sleep or a retry_after only in a comment, a string or a hidden
    // line is none.
    let hidden = "loop {\n    // tokio::time::sleep(e.retry_after()) here\n    /* sleep(e.retry_after()) */ let s = \"sleep(e.retry_after())\";\n    # tokio::time::sleep(d).await;\n    if attempt < MAX && e.is_retryable() {}\n    attempt += 1;\n}";
    assert_eq!(problems(hidden), [NO_SLEEP, DEAF, NO_JITTER]);
    let sound = "while attempt < MAX_ATTEMPTS {\n    if e.is_retryable() {\n        let jitter = step.mul_f64(random());\n        tokio::time::sleep(e.retry_after().unwrap_or(step + jitter).min(MAX_DELAY)).await;\n    }\n    attempt += 1;\n}";
    assert_eq!(problems(sound), Vec::<&str>::new());
    let ranged = "for attempt in 1..=MAX_ATTEMPTS {\n    if e.is_retryable() {\n        let delay = e.retry_after().unwrap_or(jittered(step)).min(MAX_DELAY);\n        tokio::time::sleep(delay).await;\n    }\n}";
    assert_eq!(problems(ranged), Vec::<&str>::new());
    // A count that is checked but never incremented bounds nothing.
    let stuck = "while attempt < MAX_ATTEMPTS {\n    if e.is_retryable() {\n        tokio::time::sleep(e.retry_after().unwrap_or(step).min(MAX_DELAY)).await;\n    }\n}";
    assert_eq!(problems(stuck), [UNBOUNDED]);
    // A count without jitter, and a server's wait taken whole.
    let herd = "for attempt in 1..=MAX_ATTEMPTS {\n    if e.is_retryable() {\n        tokio::time::sleep(e.retry_after().unwrap_or(step)).await;\n    }\n}";
    assert_eq!(problems(herd), [UNCAPPED, NO_JITTER]);
    // Jitter is a random value that reaches the sleep, not a name that
    // happens to contain "rand"; growth needs no spaces around its `*`.
    let named = "for attempt in 1..=MAX_ATTEMPTS {\n    if e.is_retryable() {\n        let operand = step.mul_f64(0.5);\n        let brand = rand_seed();\n        delay = (delay + operand).min(MAX_DELAY);\n        tokio::time::sleep(e.retry_after().unwrap_or(delay).min(MAX_DELAY)).await;\n    }\n}";
    assert_eq!(problems(named), [NO_JITTER]);
    let tight = "for attempt in 1..=MAX_ATTEMPTS {\n    if e.is_retryable() {\n        delay = delay*2;\n        tokio::time::sleep(e.retry_after().unwrap_or(jittered(delay)).min(MAX_DELAY) + delay).await;\n    }\n}";
    assert_eq!(problems(tight), [UNCAPPED]);
    // A poll bounded by its deadline compares with it inside the loop; a
    // deadline only declared above the loop, or named in an error
    // message, bounds nothing. A deadline loop may wait as long as the
    // server asks, but not add that wait to a time.
    let poll = |check: &str| {
        format!(
            "async fn poll(deadline: Duration) {{\n    let deadline = Instant::now() + deadline;\n    loop {{\n        let left = Duration::from_secs(3600);\n        match timeout(left, get()).await {{\n            Err(_) => return Err(\"polling deadline exceeded\".into()),\n            Ok(Err(e)) if e.is_retryable() => wait = e.retry_after().unwrap_or(delay),\n            _ => {{}}\n        }}\n{check}        sleep(wait).await;\n    }}\n}}"
        )
    };
    assert_eq!(problems(&poll("")), [UNBOUNDED, UNCAPPED]);
    assert_eq!(
        problems(&poll(
            "        if wait >= deadline.saturating_duration_since(Instant::now()) {\n            return Err(e);\n        }\n"
        )),
        Vec::<&str>::new()
    );
    assert_eq!(
        problems(&poll(
            "        if Instant::now() + wait >= deadline {\n            return Err(e);\n        }\n"
        )),
        [OVERFLOWS]
    );
    assert_eq!(
        problems(&poll(
            "        if start.elapsed() + wait > max_wait {\n            return Err(e);\n        }\n"
        )),
        [OVERFLOWS]
    );
    // A poll that sends its status request with `?` ends on the first
    // transient error, and one bounded by a deadline needs each request
    // bounded too.
    let hasty = "loop {\n    let response = client\n        .interactions()\n        .get(&id)\n        .await?;\n    if start.elapsed() > max_wait {\n        return Ok(None);\n    }\n    sleep(Duration::from_secs(5)).await;\n}";
    assert_eq!(problems(hasty), [DEAF, GIVES_UP, STALLS]);
    // Not retry loops: is_retryable() outside a loop, a map lookup in a
    // loop, a loop nested in one (the inner loop is), and a `for` over a
    // collection that paces itself.
    for text in [
        "fn f(e: &GenaiError) -> bool { e.is_retryable() }",
        "loop {\n    if let Some(v) = cache.get(&key) {\n        break v;\n    }\n}",
        "for item in items {\n    client.send(item).await?;\n    sleep(PACE).await;\n}",
    ] {
        assert!(retry_loops(&block(text)).is_empty(), "{text}");
    }
    let nested = "for id in ids {\n    let mut attempt = 0;\n    while attempt < MAX {\n        attempt += 1;\n        sleep(e.retry_after().unwrap_or(jittered(step)).min(MAX_DELAY)).await;\n        if e.is_retryable() {}\n    }\n}";
    let found = retry_loops(&block(nested));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].head, "while attempt < MAX {");
}

/// The source of a recipe the check reads: the block of `path` that holds
/// `marker` (an example's whole file).
fn recipe(path: &str, marker: &str) -> DocBlock {
    let text = std::fs::read_to_string(source_scan::root().join(path)).unwrap();
    scanned_blocks(path, &text)
        .into_iter()
        .find(|block| block.lines.iter().any(|line| line.contains(marker)))
        .unwrap_or_else(|| panic!("{path} has no block with {marker}"))
}

/// `block` with each `(from, to)` replaced, every `from` required, so a
/// recipe that changes cannot make the mutation a no-op.
fn mutated(block: &DocBlock, edits: &[(&str, &str)]) -> DocBlock {
    let mut text = block.lines.join("\n");
    for (from, to) in edits {
        assert!(text.contains(from), "{}: no {from:?} to mutate", block.at);
        text = text.replace(from, to);
    }
    DocBlock {
        at: block.at.clone(),
        lines: text.lines().map(str::to_string).collect(),
    }
}

/// The `(from, to)` replacements [`mutated`] makes.
type Edits<'a> = &'a [(&'a str, &'a str)];

/// Each defect found in a shipped recipe fails the check against the real
/// text: the request recipe without its cap, with the cap on the backoff
/// alone, or with its jitter dropped or left unused; the doc's poll and
/// the example's without the cap on their growth (or doubling by
/// addition), comparing the clock rather than the wait with the deadline,
/// applying `+` or `-` to a `Retry-After` (in a variable, inline or in a
/// closure), or sending a status request with `?`; and the poll
/// `examples/deep_research.rs` had, which ended on the first transient
/// error and never bounded a request by the time left.
#[test]
fn the_retry_loop_check_fails_each_broken_recipe() {
    let only = |block: &DocBlock| {
        let found = retry_loops(block);
        assert_eq!(found.len(), 1, "{}: {found:?}", block.at);
        found.into_iter().next().unwrap().problems
    };
    let request = recipe("src/request/mod.rs", "is_retryable");
    assert_eq!(only(&request), Vec::<&str>::new());
    let uncapped = mutated(&request, &[(".min(MAX_DELAY)", "")]);
    assert_eq!(only(&uncapped), [UNCAPPED]);
    let herd = mutated(
        &request,
        &[
            (
                "let backoff = step + step.mul_f64(random);",
                "let backoff = step;",
            ),
            (
                "let random = RandomState::new().hash_one(attempt) as f64 / u64::MAX as f64;",
                "",
            ),
        ],
    );
    assert_eq!(only(&herd), [NO_JITTER]);
    // Jitter computed but never added to the wait is none.
    let unused = mutated(
        &request,
        &[(
            "let backoff = step + step.mul_f64(random);",
            "let backoff = step;",
        )],
    );
    assert_eq!(only(&unused), [NO_JITTER]);
    // A `.min(` inside the fallback caps the backoff, not the server's
    // Retry-After.
    let inner = mutated(
        &request,
        &[(
            ".unwrap_or(backoff).min(MAX_DELAY);",
            ".unwrap_or(backoff.min(MAX_DELAY));",
        )],
    );
    assert_eq!(only(&inner), [UNCAPPED]);

    let poll = recipe("docs/AGENTS_AND_BACKGROUND.md", "is_retryable");
    assert_eq!(only(&poll), Vec::<&str>::new());
    let overflowing = mutated(
        &poll,
        &[(
            "if wait >= deadline.saturating_duration_since(Instant::now()) {",
            "if Instant::now() + wait >= deadline {",
        )],
    );
    assert_eq!(only(&overflowing), [OVERFLOWS]);
    let growing = mutated(
        &poll,
        &[(
            "delay = (delay * 2).min(Duration::from_secs(10));",
            "delay = delay * 2;",
        )],
    );
    assert_eq!(only(&growing), [UNCAPPED]);

    // The doc's poll and the example's, each broken one way at a time.
    let example = recipe("examples/deep_research.rs", "is_retryable");
    assert_eq!(only(&example), Vec::<&str>::new());
    const GUARD: &str = "if wait >= deadline.saturating_duration_since(Instant::now()) {";
    const WAIT: &str = "wait = e.retry_after().map_or(delay, |asked| asked.max(delay));";
    for poll in [&poll, &example] {
        let grow = poll
            .lines
            .iter()
            .map(|line| line.trim())
            .find(|line| line.starts_with("delay = (delay * 2).min("))
            .unwrap();
        let broken: [(Edits, &[&str]); 8] = [
            // Checking the clock, not the wait, against the deadline
            // leaves a Retry-After of u64::MAX seconds to sleep whole.
            (&[(GUARD, "if Instant::now() >= deadline {")], &[UNCAPPED]),
            // Arithmetic on a Retry-After overflows however it is
            // spelled: inline, in a closure, or subtracted.
            (
                &[(
                    WAIT,
                    "if Instant::now() + e.retry_after().unwrap_or(delay) >= deadline {\n    return Err(e.into());\n}",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked + delay);",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(GUARD, "if Instant::now() >= deadline - wait {")],
                &[OVERFLOWS],
            ),
            // A `.min(` beside the Retry-After, not on it, caps nothing.
            (
                &[
                    (
                        WAIT,
                        "wait = e.retry_after().unwrap_or(delay.min(MAX_POLL_DELAY));",
                    ),
                    (GUARD, "if Instant::now() + wait >= deadline {"),
                ],
                &[OVERFLOWS],
            ),
            // Doubling by addition is growth too.
            (&[(grow, "delay += delay;")], &[UNCAPPED]),
            (&[(grow, "delay = delay + delay;")], &[UNCAPPED]),
            // A status request sent with `?` ends the poll on its first
            // transient error, whatever else the loop retries.
            (
                &[(
                    "let mut wait = delay;",
                    "let mut wait = delay;\nlet _s = client.interactions().get(id).await?;",
                )],
                &[GIVES_UP],
            ),
        ];
        for (edits, want) in broken {
            assert_eq!(only(&mutated(poll, edits)), want, "{}: {edits:?}", poll.at);
        }
    }

    assert_eq!(
        only(&block(OLD_DEEP_RESEARCH_POLL)),
        [DEAF, GIVES_UP, STALLS]
    );
}

/// `examples/deep_research.rs`'s poll before it took the doc's.
const OLD_DEEP_RESEARCH_POLL: &str = r#"
async fn poll(
    client: &Client,
    mut response: InteractionResponse,
    max_wait: Duration,
) -> Result<Option<InteractionResponse>, Box<dyn Error>> {
    let id = response.id.clone().ok_or("interaction has no ID")?;
    let start = Instant::now();
    let mut delay = Duration::from_secs(2);
    loop {
        match response.status {
            InteractionStatus::Completed => return Ok(Some(response)),
            InteractionStatus::Failed | InteractionStatus::Cancelled => {
                return Err(format!("research ended with status {:?}", response.status).into());
            }
            _ => {}
        }
        if start.elapsed() + delay > max_wait {
            return Ok(None);
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_POLL_DELAY);
        response = client.interactions().get(&id).await?;
        println!("  {:>4}s  {:?}", start.elapsed().as_secs(), response.status);
    }
}
"#;

/// What the check reads of a line: no comment, nested or not, no string's
/// text, raw, char-quoted or spanning lines, and no hidden line.
#[test]
fn code_lines_drops_comments_and_blanks_strings() {
    let lines: Vec<String> = [
        "let a = \"x // y\"; // gone",
        "let b = '\"'; let c = '\\''; let d = r#\"q \" sleep(\"#;",
        "/* a /* nested */ comment",
        "still */ let e = \"multi",
        "line\"; fn f<'a>(x: &'a str) {}",
        "# hidden(sleep(d));",
    ]
    .map(str::to_string)
    .to_vec();
    assert_eq!(
        code_lines(&lines),
        [
            "let a = \"\"; ",
            "let b = ' '; let c = ' '; let d = \"\";",
            "",
            " let e = \"",
            "\"; fn f<'a>(x: &'a str) {}",
            "",
        ]
    );
}

#[test]
fn doc_blocks_reads_markdown_and_doc_comments() {
    let markdown = doc_blocks("a.md", "x\n```rust\nloop {}\n```\n````text\n```\ny\n````\n");
    assert_eq!(markdown.len(), 2);
    assert_eq!(markdown[0].at, "a.md:2");
    assert_eq!(markdown[0].lines, ["loop {}"]);
    assert_eq!(markdown[1].lines, ["```", "y"]);
    let rust = doc_blocks(
        "a.rs",
        "/// ```\n/// loop {}\n/// ```\nfn f() {}\n//// ```\n    //! ```no_run\n    //! a\n    //! ```\n",
    );
    assert_eq!(rust.len(), 2);
    assert_eq!(rust[0].lines, ["loop {}"]);
    assert_eq!(rust[1].at, "a.rs:6");
    assert_eq!(rust[1].lines, ["a"]);
    // rustdoc's other forms: a `/** */` or `/*! */` block, and a
    // `#[doc = "..."]` string, plain (its `\n` escapes split it) or raw.
    let forms = doc_blocks(
        "a.rs",
        "/**\n * ```\n * loop {}\n * ```\n */\nfn f() {}\n/*! ```\nb\n``` */\n#[doc = \"```\\nc \\\"d\\\"\\n```\"]\nfn g() {}\n#![doc = r#\"```\"#]\n#![doc = \"e\"]\n#![doc = r\"```\"]\n/***\n```\n*/\n",
    );
    assert_eq!(forms.len(), 4);
    assert_eq!(forms[0].at, "a.rs:2");
    assert_eq!(forms[0].lines, ["loop {}"]);
    assert_eq!(forms[1].lines, ["b"]);
    assert_eq!(forms[2].at, "a.rs:10");
    assert_eq!(forms[2].lines, ["c \"d\""]);
    assert_eq!(forms[3].lines, ["e"]);
}
