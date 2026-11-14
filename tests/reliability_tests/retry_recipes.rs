//! The retry recipes the docs and examples show: `docs/RELIABILITY.md`'s
//! backon recipe matches `examples/retry_with_backoff.rs`,
//! `docs/AGENTS_AND_BACKGROUND.md`'s poll decides as
//! `examples/deep_research.rs` does, and every retry or poll loop bounds
//! its attempts, sleeps on every path back to its head, does not give up
//! on an error `is_retryable()` accepts, honors `Retry-After` without
//! overflow, caps its delay and, bounded by a count, adds jitter, each as
//! far as a lexical reading can tell ([`retry_loops`], D-069).

use std::ops::Range;

use crate::recipe_source::{DocBlock, code_lines, doc_sources, scanned_blocks};
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
/// `.await` and `?` in turn: where it ends, and the methods it calls, each
/// `?` as `"?"`.
fn postfix(b: &[u8], mut at: usize) -> (usize, Vec<String>) {
    let mut methods = Vec::new();
    loop {
        let mut k = at;
        while b.get(k) == Some(&b' ') {
            k += 1;
        }
        if b.get(k) == Some(&b'?') {
            methods.push("?".to_string());
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

/// Whether the operand at `b[start..end]`, which holds a `Retry-After` and
/// whose method chain is `methods`, can panic on one too large: before its
/// first `.min(`, its chain calls a method [`safe_on_retry_after`] does not
/// name (`asked.mul_f64(1.5)`); or, with no `.min(` on it, `+`, `-` or `*`
/// (or `+=`, `-=`, `*=`) applies to it, each of which panics on overflow,
/// for an `Instant` and a `Duration` alike, it is in the argument of a call
/// that panics on one ([`in_overflowing_call`]), or a `checked_add`,
/// `checked_sub` or `checked_mul` on its chain, or with it as the
/// argument, has its `None` unwrapped ([`unwraps_checked`]).
fn overflows(b: &[u8], start: usize, end: usize, methods: &[String]) -> bool {
    let cap = methods.iter().position(|m| m == "min");
    methods[..cap.unwrap_or(methods.len())]
        .iter()
        .any(|m| !safe_on_retry_after(m))
        || cap.is_none()
            && (arithmetic_on(b, start, end)
                || in_overflowing_call(b, start)
                || unwraps_checked(b, start, end))
}

/// Whether `method` cannot panic on a `Retry-After` however large, called
/// on one: a comparison, a conversion (`as_secs_f64`), saturating or
/// checked arithmetic (a `checked_` call's `None` unwrapped is
/// [`unwraps_checked`]'s), or one of `Option`'s adapters but `unwrap` and
/// `expect`. `Duration`'s `add`, `mul_f64`, `div_f64` and the rest panic
/// on a result too large; a method this does not name counts as one that
/// can.
fn safe_on_retry_after(method: &str) -> bool {
    const PREFIXES: [&str; 8] = [
        "as_",
        "subsec_",
        "is_",
        "saturating_",
        "checked_",
        "map",
        "unwrap_or",
        "ok_or",
    ];
    const NAMES: [&str; 19] = [
        "?",
        "min",
        "max",
        "abs_diff",
        "and_then",
        "filter",
        "or",
        "or_else",
        "clone",
        "copied",
        "cloned",
        "cmp",
        "partial_cmp",
        "eq",
        "ne",
        "lt",
        "le",
        "gt",
        "ge",
    ];
    PREFIXES.iter().any(|prefix| method.starts_with(prefix)) || NAMES.contains(&method)
}

/// The calls that panic when what they compute overflows: the operators'
/// methods, `Duration`'s float scalings, and its constructors from a float
/// or from minutes or hours.
const OVERFLOWING_CALLS: [&str; 14] = [
    "add",
    "sub",
    "mul",
    "add_assign",
    "sub_assign",
    "mul_assign",
    "mul_f32",
    "mul_f64",
    "div_f32",
    "div_f64",
    "from_secs_f32",
    "from_secs_f64",
    "from_mins",
    "from_hours",
];

/// Whether the operand that starts at `b[start]` is in the argument of a
/// call [`OVERFLOWING_CALLS`] names, however deep within its statement
/// (`Duration::from_secs_f64(asked.as_secs_f64())`, `now.add(wait)`).
fn in_overflowing_call(b: &[u8], start: usize) -> bool {
    let mut depth = 0usize;
    for k in (0..start).rev() {
        match b[k] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            // The block or statement it is in ends the call it can be in.
            b'{' | b';' if depth == 0 => return false,
            b'(' => {
                let before = b[..k].trim_ascii_end();
                let name =
                    before.len() - before.iter().rev().take_while(|&&c| ident_byte(c)).count();
                if OVERFLOWING_CALLS
                    .iter()
                    .any(|call| call.as_bytes() == &before[name..])
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// The `checked_` arithmetic whose `None` panics when unwrapped.
const CHECKED: [&str; 3] = ["checked_add(", "checked_sub(", "checked_mul("];

/// Whether a `checked_add`, `checked_sub` or `checked_mul` on the operand
/// at `b[start..end]` (on its chain: `wait.checked_add(d).unwrap()`), or
/// with it as the whole argument (`now.checked_add(wait).expect(..)`), is
/// followed by `unwrap()` or `expect(`: the panic `+` has, spelled out.
fn unwraps_checked(b: &[u8], start: usize, end: usize) -> bool {
    let unwrapped = |methods: &[String]| methods.iter().any(|m| m == "unwrap" || m == "expect");
    let chain = String::from_utf8_lossy(&b[start..end]);
    let on_chain = CHECKED.iter().any(|call| {
        chain.match_indices(call).any(|(at, _)| {
            let rest = &chain[at..];
            rest.contains(".unwrap()") || rest.contains(".expect(")
        })
    });
    // The argument: the `(` that opens just before it, past spaces, and
    // the `)` just after it.
    let open = b[..start].iter().rposition(|c| *c != b' ');
    let close = b[end..].iter().position(|c| *c != b' ').map(|k| end + k);
    let in_call = match (open, close) {
        (Some(open), Some(close)) if b[open] == b'(' && b[close] == b')' => CHECKED
            .iter()
            .any(|call| b[..=open].ends_with(call.as_bytes()))
            .then(|| postfix(b, close + 1).1),
        _ => None,
    };
    on_chain || in_call.is_some_and(|methods| unwrapped(&methods))
}

/// Whether `+`, `-` or `*` (or `+=`, `-=`, `*=`) applies to the operand at
/// `b[start..end]`.
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

/// The operands in `text` that hold a `Retry-After`: each `retry_after()`
/// call, with its receiver, and each name in `held`, each with its method
/// chain. As `(start, end, methods)`: its byte range, and the methods its
/// chain calls after the call or the name ([`postfix`]).
fn retry_after_operands(text: &str, held: &[String]) -> Vec<(usize, usize, Vec<String>)> {
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
        found.push((receiver_start(b, at - 1), end, methods));
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
            found.push((at, end, methods));
        }
    }
    found
}

/// The operands in `text` that hold an uncapped `Retry-After`: those of
/// [`retry_after_operands`] whose chain calls no `.min(`. As byte ranges.
fn uncapped_operands(text: &str, held: &[String]) -> Vec<(usize, usize)> {
    retry_after_operands(text, held)
        .into_iter()
        .filter(|(.., methods)| !methods.iter().any(|m| m == "min"))
        .map(|(start, end, _)| (start, end))
        .collect()
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

/// `k` moved past any whitespace in `b`.
fn skip_spaces(b: &[u8], k: usize) -> usize {
    k + b.get(k..).map_or(0, |rest| {
        rest.iter().take_while(|c| c.is_ascii_whitespace()).count()
    })
}

/// The identifier that starts at `b[k]`, if one does there (not inside a
/// longer one); empty otherwise.
fn word_at(b: &[u8], k: usize) -> &[u8] {
    if k >= b.len() || (k > 0 && ident_byte(b[k - 1])) {
        return &[];
    }
    let len = b[k..].iter().take_while(|&&c| ident_byte(c)).count();
    &b[k..k + len]
}

/// The `{` that opens the block of the `if`, `match` or loop whose
/// keyword is at or before `k`: the first one from `k` to `end` outside
/// parentheses and brackets.
fn opener(b: &[u8], k: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (j, &c) in b.iter().enumerate().take(end).skip(k) {
        match c {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'{' if depth == 0 => return Some(j),
            _ => {}
        }
    }
    None
}

/// Where the block-like expression at `k` (a block, an `if` with its
/// `else` chain, a `match` or a loop) ends, just after its last `}`.
fn block_end(b: &[u8], k: usize, end: usize) -> Option<usize> {
    let mut close = if b.get(k) == Some(&b'{') {
        close_of(b, k)?
    } else if matches!(
        word_at(b, k),
        b"if" | b"match" | b"loop" | b"while" | b"for"
    ) {
        close_of(b, opener(b, k, end)?)?
    } else {
        return None;
    };
    loop {
        let next = skip_spaces(b, close + 1);
        if word_at(b, next) != b"else" {
            return Some((close + 1).min(end));
        }
        close = close_of(b, opener(b, next, end)?)?;
    }
}

/// The arms of the `match` whose block is `b[open..=close]`: each one's
/// pattern with its guard, and its body, as byte ranges.
fn match_arms(b: &[u8], open: usize, close: usize) -> Vec<(Range<usize>, Range<usize>)> {
    let mut arms = Vec::new();
    let mut k = skip_spaces(b, open + 1);
    while k < close {
        let mut depth = 0usize;
        let Some(arrow) = (k..close).find(|&j| {
            match b[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
            depth == 0 && b[j..].starts_with(b"=>")
        }) else {
            break;
        };
        let body = skip_spaces(b, arrow + 2);
        // A block-like body ends at its `}`, unless a call or a `?`
        // follows; any other at the `,` that ends the arm.
        let end = block_end(b, body, close)
            .filter(|&end| !matches!(b.get(skip_spaces(b, end)), Some(b'.' | b'?')))
            .unwrap_or_else(|| {
                let mut depth = 0usize;
                (body..close)
                    .find(|&j| {
                        match b[j] {
                            b'(' | b'[' | b'{' => depth += 1,
                            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                            _ => {}
                        }
                        depth == 0 && b[j] == b','
                    })
                    .unwrap_or(close)
            });
        arms.push((k..arrow, body..end));
        k = skip_spaces(b, end);
        if b.get(k) == Some(&b',') {
            k = skip_spaces(b, k + 1);
        }
    }
    arms
}

/// Whether the `{` at `open` starts a closure's or an async block's body,
/// whose `return` and `?` are its own.
fn closure_body(b: &[u8], open: usize) -> bool {
    let before = b[..open].trim_ascii_end();
    before.ends_with(b"|") || before.ends_with(b"move") || before.ends_with(b"async")
}

/// Where the body of a closure whose parameters open at `b[k]` ends, when
/// that body is not a block (`|| sleep(wait)`): at the `;` or `,` that
/// ends it, or the bracket that closes around it. A `|` after an operand
/// is an or, not a closure; a block body is read as a block
/// ([`closure_body`]).
fn bare_closure(b: &[u8], k: usize, end: usize) -> Option<usize> {
    if b[k] != b'|' {
        return None;
    }
    let before = b[..k].trim_ascii_end();
    let opens = before.ends_with(b"move")
        || matches!(
            before.last(),
            None | Some(b'=' | b'(' | b',' | b'{' | b'[' | b';' | b'>')
        );
    if !opens {
        return None;
    }
    let params = (k + 1..end).find(|&j| b[j] == b'|')?;
    let body = skip_spaces(b, params + 1);
    if b.get(body) == Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let close = (body..end)
        .find(|&j| match b[j] {
            b'(' | b'[' | b'{' => {
                depth += 1;
                false
            }
            b')' | b']' | b'}' if depth == 0 => true,
            b')' | b']' | b'}' => {
                depth -= 1;
                false
            }
            b';' | b',' => depth == 0,
            _ => false,
        })
        .unwrap_or(end);
    Some(close)
}

/// Whether `guard`, a match arm's pattern and guard or an `if`'s
/// condition, holds only for an error a retry can fix: it calls
/// `is_retryable()` with no `!` before it.
fn retryable_branch(guard: &[u8]) -> bool {
    let text = String::from_utf8_lossy(guard);
    text.match_indices("is_retryable").any(|(at, _)| {
        let mut k = if at > 0 && guard[at - 1] == b'.' {
            receiver_start(guard, at - 1)
        } else {
            at - guard[..at]
                .iter()
                .rev()
                .take_while(|&&c| ident_byte(c) || c == b':')
                .count()
        };
        while k > 0 && matches!(guard[k - 1], b' ' | b'(') {
            k -= 1;
        }
        k == 0 || guard[k - 1] != b'!'
    })
}

/// At a point in a loop's body: `None` when no path reaches it, otherwise
/// whether every path that does has passed a sleep.
type Slept = Option<bool>;

/// Where two paths meet: each that reaches must have slept.
fn join(a: Slept, b: Slept) -> Slept {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(a && b),
    }
}

/// What the paths through a loop's body do ([`Paths::walk`]): whether one
/// goes back to the loop's head with no sleep on it, and whether one
/// returns or breaks in the arm or branch that `is_retryable()` guards.
#[derive(Default)]
struct Paths {
    sleepless: bool,
    gives_up: bool,
}

impl Paths {
    /// Reads `b[k..end]` from `slept` and returns the state at `end`. A
    /// `sleep(` call sleeps; `continue` goes back to the head (so must
    /// have slept); `return` and `break` leave; an `if` or a `match` runs
    /// one branch or arm, each read from the state before it; a nested
    /// loop's paths, and a closure's body ([`bare_closure`] when not a
    /// block), are their own.
    fn walk(
        &mut self,
        b: &[u8],
        mut k: usize,
        end: usize,
        mut slept: Slept,
        retrying: bool,
    ) -> Slept {
        while k < end {
            if b[k] == b'{' {
                let close = close_of(b, k).map_or(end, |close| close.min(end));
                if !closure_body(b, k) {
                    slept = self.walk(b, k + 1, close, slept, retrying);
                }
                k = close + 1;
                continue;
            }
            if let Some(body_end) = bare_closure(b, k, end) {
                k = body_end;
                continue;
            }
            let word = word_at(b, k);
            let after = k + word.len().max(1);
            match word {
                b"if" => (k, slept) = self.branch(b, k, end, slept, retrying),
                b"match" => {
                    let Some(open) = opener(b, after, end) else {
                        return slept;
                    };
                    let close = close_of(b, open).map_or(end, |close| close.min(end));
                    let arms = match_arms(b, open, close);
                    if !arms.is_empty() {
                        slept = arms.into_iter().fold(None, |met, (guard, body)| {
                            let retries = retrying || retryable_branch(&b[guard]);
                            join(met, self.walk(b, body.start, body.end, slept, retries))
                        });
                    }
                    k = close + 1;
                }
                b"loop" | b"while" | b"for" => {
                    k = block_end(b, k, end).unwrap_or(end);
                }
                // A `let ... else` block leaves; the path goes on without it.
                b"else" => {
                    let open = skip_spaces(b, after);
                    k = match close_of(b, open).filter(|_| b[open] == b'{') {
                        Some(close) => {
                            self.walk(b, open + 1, close.min(end), slept, retrying);
                            close + 1
                        }
                        None => after,
                    };
                }
                b"return" | b"break" => {
                    self.gives_up |= retrying && slept.is_some();
                    slept = None;
                    k = after;
                }
                b"continue" => {
                    self.sleepless |= slept == Some(false);
                    slept = None;
                    k = after;
                }
                b"sleep" if b.get(skip_spaces(b, after)) == Some(&b'(') => {
                    slept = slept.map(|_| true);
                    k = after;
                }
                _ => k = after,
            }
        }
        slept
    }

    /// The `if` at `k` and its `else` chain: where it ends, and the state
    /// after it. A branch whose condition calls `is_retryable()` is the
    /// retrying one; its `else` is not.
    fn branch(
        &mut self,
        b: &[u8],
        k: usize,
        end: usize,
        slept: Slept,
        retrying: bool,
    ) -> (usize, Slept) {
        let Some(open) = opener(b, k + 2, end) else {
            return (end, slept);
        };
        let close = close_of(b, open).map_or(end, |close| close.min(end));
        let retries = retrying || retryable_branch(&b[k + 2..open]);
        let then = self.walk(b, open + 1, close, slept, retries);
        let next = skip_spaces(b, close + 1);
        if word_at(b, next) != b"else" {
            return (close + 1, join(then, slept));
        }
        let other = skip_spaces(b, next + "else".len());
        if word_at(b, other) == b"if" {
            let (to, otherwise) = self.branch(b, other, end, slept, retrying);
            return (to, join(then, otherwise));
        }
        let close = close_of(b, other).map_or(end, |close| close.min(end));
        let otherwise = self.walk(b, other + 1, close, slept, retrying);
        (close + 1, join(then, otherwise))
    }
}

/// Whether an `.await` in `text` has a `?` on the chain after it
/// (`.await?`, `.await.map_err(..)?`), or after the parentheses that
/// group it (`(fut.await)?`): an exit on the first error.
fn awaits_with_try(text: &str) -> bool {
    let b = text.as_bytes();
    text.match_indices(".await").any(|(at, _)| {
        let mut after = at + ".await".len();
        if b.get(after).is_some_and(|&c| ident_byte(c)) {
            return false;
        }
        loop {
            let (end, methods) = postfix(b, after);
            if methods.iter().any(|m| m == "?") {
                return true;
            }
            // Out of a grouping `(..)`, not a call's or a tuple's.
            let close = skip_spaces(b, end);
            let grouped = b.get(close) == Some(&b')')
                && open_of(b, close).is_some_and(|open| {
                    !matches!(
                        b[..open].trim_ascii_end().last(),
                        Some(&c) if ident_byte(c) || matches!(c, b')' | b']' | b'>' | b'!')
                    )
                });
            if !grouped {
                return false;
            }
            after = close + 1;
        }
    })
}

const UNBOUNDED: &str = "does not bound its attempts by a count or a deadline";
const NO_SLEEP: &str = "does not sleep on every path back to its head: a `continue`, or the \
     end of its body, that no sleep comes before";
const DEAF: &str = "ignores the server's retry_after()";
const GIVES_UP: &str = "ends on the first transient error: a `?` on an awaited chain, a \
     return or break in its is_retryable() arm, or no is_retryable() at all";
const STALLS: &str = "does not bound each request by the time left (timeout(left, ..))";
const UNCAPPED: &str = "sleeps with no ceiling: growth needs a .min(), and a retry_after() a \
     .min() or a comparison of the wait with the time left";
const OVERFLOWS: &str = "can panic on a Retry-After too large: calls a method on a retry_after() \
     that can (mul_f64, add, ...) before any .min() on it, or, on an uncapped one, applies +, - \
     or *, passes it to a call that can (from_secs_f64, add, ...) or unwraps checked_ \
     arithmetic on it";
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
/// - sleep on every path back to its head ([`Paths`]): no `continue`
///   that no sleep comes before, in its arm or a block around it, and no
///   way through to the end of the body that misses one (a sleep in a
///   closure, its body in braces or not, is the closure's);
/// - end on no transient error: no `?` on the chain after an `.await`, or
///   after the parentheses around one ([`awaits_with_try`]);
/// - when it calls `is_retryable()` or awaits a `.get(`, read the server's
///   `retry_after()`, and retry what `is_retryable()` says to: no `return`
///   or `break` in the arm or `if` branch that `is_retryable()` guards (not
///   negated), and a poll calls `is_retryable()`;
/// - when a deadline bounds a poll, bound each request by the time left
///   (`timeout(`);
/// - cap the delay it sleeps: growth reaches the sleep through a `.min(`,
///   and so does a `retry_after()`, unless the loop compares the variable
///   that holds it with the time left (a line that names the variable and
///   a deadline, an `elapsed()` or a value built from one, and compares);
/// - call nothing that can panic on a `retry_after()` too large, inline, in
///   a variable or in a closure along its chain ([`overflows`]): no method
///   but a safe one on its chain before any `.min(`, and, uncapped, no `+`,
///   `-` or `*`, no call that overflows with it in its argument
///   (`from_secs_f64(`, `add(`, ...), and no `checked_add`, `checked_sub`
///   or `checked_mul` on one or of one whose `None` is unwrapped;
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
            let bytes = text.as_bytes();
            let mut paths = Paths::default();
            if let Some(open) = opener(bytes, at, bytes.len()) {
                let close = close_of(bytes, open).unwrap_or(bytes.len());
                // The end of the body goes back to the head too.
                let end = paths.walk(bytes, open + 1, close, Some(false), false);
                paths.sleepless |= end == Some(false);
            }
            let mut problems = Vec::new();
            if !(counted || timed) {
                problems.push(UNBOUNDED);
            }
            if sleeps.is_empty() || paths.sleepless {
                problems.push(NO_SLEEP);
            }
            if requests && !has("retry_after()") {
                problems.push(DEAF);
            }
            if (calls.polls && !has("is_retryable")) || awaits_with_try(&text) || paths.gives_up {
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
            if retry_after_operands(&text, &held)
                .into_iter()
                .any(|(from, to, methods)| overflows(bytes, from, to, &methods))
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

/// A retry or poll loop shown in the docs or an example bounds its
/// attempts, sleeps on every path back to its head, gives up on no error
/// `is_retryable()` accepts, honors the server's `Retry-After` without
/// overflow, caps its delay and, bounded by a count, adds jitter
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

// =============================================================================
// The background poll in docs/AGENTS_AND_BACKGROUND.md and its runnable example
// =============================================================================

/// `text` with its whitespace collapsed, and a trailing comma and the
/// braces of a block dropped.
fn flat(text: &str) -> String {
    let text = text.trim().trim_end_matches(',').trim();
    let text = text
        .strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .unwrap_or(text);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the poll in `block` decides, as `(when, then)` in order: each arm
/// of its `match` on a status request's outcome, with the arm that holds
/// its `match` on the status split in two (what the arm does around that
/// `match`, then each of its arms), then the first `if` after them, its
/// comparison with the deadline. Comments, string text and print
/// statements don't count.
fn poll_decisions(block: &DocBlock) -> Vec<(String, String)> {
    let mut code = code_lines(&block.lines).join(" ");
    for print in ["eprintln!(", "println!("] {
        while let Some(at) = code.find(print) {
            let b = code.as_bytes();
            let close = close_of(b, at + print.len() - 1).expect("a print closes");
            let end = close + 1 + usize::from(b.get(close + 1) == Some(&b';'));
            code.replace_range(at..end, "");
        }
    }
    let b = code.as_bytes();
    let block_of = |at: usize| {
        let open = opener(b, at, b.len()).expect("a block follows");
        (open, close_of(b, open).expect("the block closes"))
    };
    let mut decided = Vec::new();
    let outcome = code
        .find("match timeout(")
        .unwrap_or_else(|| panic!("{}: the poll matches each request's outcome", block.at));
    let (open, close) = block_of(outcome);
    for (when, then) in match_arms(b, open, close) {
        let Some(status) = code[then.clone()].find("match response.status") else {
            decided.push((flat(&code[when]), flat(&code[then])));
            continue;
        };
        let (open, close) = block_of(then.start + status);
        // What else the arm does, around the `match` on the status.
        let rest = format!(
            "{} {}",
            &code[then.start..then.start + status],
            &code[close + 1..then.end]
        );
        decided.push((flat(&code[when.clone()]), flat(&rest)));
        for (when, then) in match_arms(b, open, close) {
            decided.push((flat(&code[when]), flat(&code[then])));
        }
    }
    let guard = (close..b.len())
        .find(|&k| word_at(b, k) == b"if")
        .unwrap_or_else(|| panic!("{}: the poll compares with its deadline", block.at));
    let (open, close) = block_of(guard);
    decided.push((flat(&code[guard..open]), flat(&code[open..=close])));
    decided
}

/// The example's decisions as the doc's poll would make them, the one
/// difference the doc states undone: the example returns an `Option`,
/// `Some` once the run completes, and `None` where the doc's poll gives up
/// at the deadline (a request cut off by the time left, or a wait that
/// would outlast it) with an error.
fn example_as_doc(decided: Vec<(String, String)>) -> Vec<(String, String)> {
    decided
        .into_iter()
        .map(|(when, then)| {
            let then = if when == "Err(_)" || when.contains("deadline") {
                then.replace("Ok(None)", "Err(\"\".into())")
            } else {
                then.replace("Ok(Some(response))", "Ok(response)")
            };
            (when, then)
        })
        .collect()
}

/// Whether `block` builds every `deadline` it assigns with `checked_add`,
/// its `None` not unwrapped, so a caller's deadline too far off is an
/// error, not a panic.
fn checked_deadline(block: &DocBlock) -> bool {
    let deadlines: Vec<String> = assignments(&code_lines(&block.lines))
        .into_iter()
        .filter(|(name, _)| name == "deadline")
        .map(|(_, rhs)| rhs)
        .collect();
    !deadlines.is_empty()
        && deadlines.iter().all(|rhs| {
            let b = rhs.as_bytes();
            let handled: Vec<bool> = rhs
                .match_indices(".checked_add(")
                .map(|(at, _)| {
                    close_of(b, at + ".checked_add".len()).is_some_and(|close| {
                        !postfix(b, close + 1)
                            .1
                            .iter()
                            .any(|m| m == "unwrap" || m == "expect")
                    })
                })
                .collect();
            !handled.is_empty() && handled.iter().all(|&ok| ok)
        })
}

/// What `block` assigns its backoff, `delay`, in order: its first value,
/// then its growth with its ceiling.
fn backoff(block: &DocBlock) -> Vec<String> {
    assignments(&code_lines(&block.lines))
        .into_iter()
        .filter(|(name, _)| name == "delay")
        .map(|(_, rhs)| rhs)
        .collect()
}

/// `docs/AGENTS_AND_BACKGROUND.md`'s poll and `examples/deep_research.rs`,
/// which the doc says runs it, decide alike: the same terminal statuses,
/// the same retryable arm and the same comparison with the deadline, with
/// only the difference the doc states, and back off alike. The example
/// once dropped statuses the doc's poll ends on, and once capped its
/// backoff at 15 s to the doc's 10 s, and nothing compared the two. Each
/// builds its deadline with `checked_add`, its `None` not unwrapped.
#[test]
fn the_poll_in_agents_and_background_md_matches_its_example() {
    let doc = recipe("docs/AGENTS_AND_BACKGROUND.md", "is_retryable");
    let example = recipe("examples/deep_research.rs", "is_retryable");
    let decided = poll_decisions(&doc);
    assert_eq!(decided, example_as_doc(poll_decisions(&example)));
    assert_eq!(backoff(&doc), backoff(&example));
    assert_eq!(backoff(&doc).len(), 2, "{:?}", backoff(&doc));
    for when in [
        "Err(_)",
        "InteractionStatus::Completed",
        "InteractionStatus::Failed | InteractionStatus::Cancelled | InteractionStatus::Incomplete | InteractionStatus::BudgetExceeded",
        "Ok(Err(e)) if e.is_retryable()",
        "if wait >= deadline.saturating_duration_since(Instant::now())",
    ] {
        assert!(
            decided.iter().any(|(w, _)| w == when),
            "no {when}: {decided:#?}"
        );
    }
    for poll in [&doc, &example] {
        assert!(checked_deadline(poll), "{}: an unchecked deadline", poll.at);
    }
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

#[path = "retry_guard_cases.rs"]
mod guard_cases;
