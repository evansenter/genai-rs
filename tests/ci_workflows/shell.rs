//! How `tests/ci_workflows/` reads a step's shell script: [`run_script`]
//! takes the `run:` value YAML reads (`super::yaml`) and splits it into
//! command lines, and [`shell_masks`] names what in a guarded step's
//! script lets a failing command pass the step. The runner splices each
//! `${{ }}` expression's value into that string before bash reads it, so
//! [`shell_masks`] reads the script with each matrix value it can splice
//! in, and rejects any other expression ([`spliced`]).
//!
//! The script itself is read as text, against the forms named here. That
//! catches the ways a step is known, or likely, to be weakened; a script
//! written to compute its status out of the guard's sight (a condition
//! that reads a variable but never holds, a loop over an empty list, a
//! variable set by a command it does not read, a command word spelled so
//! the text does not read it as one, such as `"alias"` or `al''ias`)
//! could still pass it (D-058).

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;
use yaml_rust2::yaml::Hash;

use super::yaml;

/// A regex compiled once, where it is used.
macro_rules! regex {
    ($source:expr) => {{
        static CELL: OnceLock<Regex> = OnceLock::new();
        CELL.get_or_init(|| Regex::new($source).expect("a shell pattern"))
    }};
}

/// What may come before a command: the line's start, an operator, an
/// opening brace or parenthesis, or `then`, `do` or `else`.
const AT_COMMAND: &str = r"(?:^|[;&|({]|\b(?:then|do|else)\b)\s*";

/// The script of a step's `run:`, one command line per entry: the value
/// YAML reads (a block's lines, a plain value's continuation lines and a
/// folded block's lines joined as YAML joins them), each line trimmed,
/// blank and comment lines dropped and `\` continuations joined. Empty for
/// a step with no `run:`, or one YAML cannot read ([`shell_masks`] names
/// that).
pub(super) fn run_script(step: &str) -> Vec<String> {
    match yaml::run_value(step) {
        Ok(Some(run)) => script_lines(&run),
        _ => Vec::new(),
    }
}

/// A `run:` value's command lines, as [`run_script`] gives them.
pub(super) fn script_lines(run: &str) -> Vec<String> {
    let mut script = Vec::new();
    let mut joined = String::new();
    for line in run.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.strip_suffix('\\') {
            Some(head) => {
                joined.push_str(head.trim_end());
                joined.push(' ');
            }
            None => {
                joined.push_str(line);
                script.push(std::mem::take(&mut joined));
            }
        }
    }
    if !joined.is_empty() {
        script.push(joined);
    }
    script
}

/// `line` with what is inside its quotes, and each backslash-escaped
/// character, blanked to `_` byte for byte (the quotes kept), so an
/// operator in a quoted argument or message is not read as the shell's,
/// and an offset in the result is the same offset in `line`. A command
/// substitution's opener in double quotes (`$(` or a backtick) is kept:
/// the shell runs it there too.
pub(super) fn blank_quotes(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let blank = |out: &mut String, c: char| out.extend(std::iter::repeat_n('_', c.len_utf8()));
    let mut quote: Option<char> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match quote {
            None | Some('"') if c == '\\' => {
                blank(&mut out, c);
                if let Some(next) = next {
                    blank(&mut out, next);
                    i += 1;
                }
            }
            None => {
                out.push(c);
                if c == '\'' || c == '"' {
                    quote = Some(c);
                }
            }
            Some(open) if c == open => {
                out.push(c);
                quote = None;
            }
            Some('"') if c == '`' => out.push(c),
            Some('"') if c == '$' && next == Some('(') && chars.get(i + 2) != Some(&'(') => {
                out.push_str("$(");
                i += 1;
            }
            Some(_) => blank(&mut out, c),
        }
        i += 1;
    }
    out
}

/// Whether a line, its quotes blanked, has a pipe (`|`, not `||`).
pub(super) fn has_pipe(bare: &str) -> bool {
    let chars: Vec<char> = bare.chars().collect();
    chars.iter().enumerate().any(|(i, c)| {
        *c == '|' && (i == 0 || chars[i - 1] != '|') && chars.get(i + 1) != Some(&'|')
    })
}

/// Whether a line, its quotes blanked, runs a command in the background
/// (an `&` that is neither `&&` nor part of a redirection).
fn backgrounds(bare: &str) -> bool {
    let chars: Vec<char> = bare.chars().collect();
    chars.iter().enumerate().any(|(i, c)| {
        *c == '&'
            && !(i > 0 && matches!(chars[i - 1], '&' | '>' | '<'))
            && !matches!(chars.get(i + 1), Some('&' | '>'))
    })
}

/// `set +e`, `set +o errexit` or `set +o pipefail`, alone or among other
/// flags, or a `shopt` of `errexit` or `pipefail`: what turns off the
/// checks a guarded script starts with.
const CHECKS_OFF: &str =
    r"\bset\s[^;&|]*\+(\w*e|o\s+(errexit|pipefail))|\bshopt\s[^;&|]*\b(errexit|pipefail)\b";

/// Each `set` and `shopt` command in `line` (`blank` is it with its quotes
/// blanked), with its quotes and backslashes dropped, as the builtin reads
/// its arguments (`set -o "posix"` is `set -o posix`), joined by `; `.
fn option_commands(line: &str, blank: &str) -> String {
    regex!(&format!(r"{AT_COMMAND}((?:set|shopt)\b)"))
        .captures_iter(blank)
        .filter_map(|found| found.get(1))
        .map(|name| {
            let end = blank[name.end()..]
                .find([';', '|', '&'])
                .map_or(blank.len(), |at| name.end() + at);
            line[name.start()..end]
                .chars()
                .filter(|c| !matches!(c, '\'' | '"' | '\\'))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Whether a line turns `-e` or `pipefail` off, its options quoted or not.
pub(super) fn turns_checks_off(line: &str) -> bool {
    let blank = blank_quotes(line);
    regex!(CHECKS_OFF).is_match(&blank)
        || regex!(CHECKS_OFF).is_match(&option_commands(line, &blank))
}

/// `$(grep -c .. || true)`: a count whose exit status 1 means zero, the
/// one `|| true` a guarded script may have.
fn grep_count() -> &'static Regex {
    regex!(r"\$\(grep\s+-c\w*\s[^()]*\|\|\s*true\)")
}

/// The one count of skip markers a guarded script may make, its grep
/// pattern and the log it reads captured.
pub(super) const SKIP_COUNT: &str = r"^skips=\$\(grep -cE '([^']*)' (\S+) \|\| true\)$";

/// The last statement of a script that fails on a list of failures.
const FAILED_CHECK: &str = "if [ \"${#failed[@]}\" -gt 0 ]; then";

/// What of a line a mask pattern reads.
#[derive(Clone, Copy)]
enum Reads {
    /// The line with its quotes blanked.
    Bare,
    /// That, and each `set` or `shopt` command with its quotes dropped
    /// ([`option_commands`]), as the builtin reads its options.
    Options,
    /// The line as written, quotes and all: a name bash reads wherever it
    /// is assigned, quoted or not (`declare "BASH_ALIASES[x]=y"`).
    Line,
}

/// The shell-level masks [`shell_masks`] rejects by pattern, each with
/// what of a line it reads.
fn mask_patterns() -> &'static [(Regex, &'static str, Reads)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str, Reads)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            (
                r"(^|;|&&|\|\||\bthen|\bdo|\belse)\s*(true|:)\s*(;|$|\bfi\b|\bdone\b)".to_owned(),
                "a `true` or `:` command (`; true`), which leaves the step's status 0 when \
                 it runs last",
                Reads::Bare,
            ),
            (
                CHECKS_OFF.to_owned(),
                "`set +e`, `set +o pipefail` or a `shopt` of either, which lets a failing \
                 command pass",
                Reads::Options,
            ),
            (
                r"(^|;|&&|\|\|)\s*trap\b".to_owned(),
                "a `trap`, which can rewrite the step's exit status",
                Reads::Bare,
            ),
            (
                r"(^|;|&&|\|\||\bthen|\bdo|\belse)\s*!\s".to_owned(),
                "a `!`, which inverts a command's status and is exempt from `-e`",
                Reads::Bare,
            ),
            (
                r"&&".to_owned(),
                "a `&&` list, where `-e` passes over a failure before its last command",
                Reads::Bare,
            ),
            (
                r"\b(if|elif|while|until)\s+(!\s*)?[^\[\s]".to_owned(),
                "a command as an `if` or loop condition, where `-e` passes over its \
                 failure (a `[ .. ]` test is the only condition allowed)",
                Reads::Bare,
            ),
            (
                format!(r"{AT_COMMAND}exec\b"),
                "an `exec`, which replaces the script, so nothing after it runs",
                Reads::Bare,
            ),
            (
                format!(
                    r"{AT_COMMAND}(eval|source|\.)\s|(^|[\s;&|(])(ba|da|k|z)?sh\s+(-\S+\s+)*-\w*c\b"
                ),
                "a script the guard cannot read (`eval`, `source`, `bash -c`), which can \
                 hide a mask",
                Reads::Bare,
            ),
            (
                format!(r"{AT_COMMAND}(function\s+\w|[\w-]+\s*\(\s*\))"),
                "a function, whose body the guard does not read as the step's",
                Reads::Bare,
            ),
            (
                format!(
                    r"{AT_COMMAND}alias\b|\b(?:set|shopt)\s[^;&|]*\b(?:expand_aliases|posix)\b"
                ),
                "an `alias`, or alias expansion turned on (`shopt -s expand_aliases`, or \
                 `set -o posix`, whose POSIX mode expands aliases in a script), under which \
                 a command the guard reads runs another (`alias cargo=true`), as a \
                 function would",
                Reads::Options,
            ),
            (
                r"\bBASH_ALIASES\b".to_owned(),
                "an `alias` set through `BASH_ALIASES`, under which a command the guard \
                 reads runs another, as a function would",
                Reads::Line,
            ),
            (
                format!(r"{AT_COMMAND}(let\s|\(\()"),
                "arithmetic (`let`, `(( ))`), which can set a status out of the guard's sight",
                Reads::Bare,
            ),
        ]
        .into_iter()
        .map(|(pattern, why, reads)| (Regex::new(&pattern).expect("a mask pattern"), why, reads))
        .collect()
    })
}

/// A `${{ }}` expression in a `run:` value, which the runner evaluates and
/// splices into the script before bash reads any of it: `(its text, the
/// expression within it, trimmed)`. One with no closing `}}` runs to the
/// value's end.
fn expressions(run: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut rest = run;
    while let Some(start) = rest.find("${{") {
        let after = &rest[start + 3..];
        let (inner, len) = match after.find("}}") {
            Some(end) => (&after[..end], 3 + end + 2),
            None => (after, rest.len() - start),
        };
        found.push((&rest[start..start + len], inner.trim()));
        rest = &rest[start + len..];
    }
    found
}

/// A matrix value the guard splices into a script: plain words, which
/// hold no shell syntax.
const PLAIN_WORDS: &str = r"^[\w./,=+-]+(?: +[\w./,=+-]+)*$";

/// What the guard reads in place of an expression it cannot evaluate, so
/// the rest of the script is still read.
const UNREAD: &str = "UNREAD_EXPRESSION";

/// The scripts bash may run for the `run:` value `run`, as the runner
/// splices its expressions in, and the expressions the guard cannot read,
/// each named. A `${{ matrix.<key> }}` is spliced in with each value
/// `job`'s matrix gives the key, one script for each, and each value must
/// be plain words ([`PLAIN_WORDS`]). Any other expression, or a matrix
/// value the guard cannot read, is rejected: its value, written into the
/// script, could be any shell (`${{ '|| true' }}` is `|| true` to bash).
fn spliced(run: &str, job: Option<&Hash>) -> (Vec<String>, Vec<String>) {
    let mut scripts = vec![run.to_owned()];
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    let matrix_key = regex!(r"^matrix\.([A-Za-z_][\w-]*)$");
    let plain_words = regex!(PLAIN_WORDS);
    for (text, expression) in expressions(run) {
        if !seen.insert(text) {
            continue;
        }
        let unread = |why: &str| {
            format!(
                "a `{text}` expression, {why}, which the runner splices into the script \
                 before bash reads it, so bash can run what the guard never read"
            )
        };
        let key = matrix_key.captures(expression).map(|key| key[1].to_owned());
        let values = match (key, job) {
            (Some(key), Some(job)) => match yaml::matrix_values(job, &key) {
                Ok(values) => {
                    let unplain: Vec<&String> = values
                        .iter()
                        .filter(|value| !plain_words.is_match(value))
                        .collect();
                    if !unplain.is_empty() {
                        found.push(unread(&format!(
                            "whose matrix values {unplain:?} are not plain words"
                        )));
                    }
                    values
                }
                Err(e) => {
                    found.push(unread(&format!(
                        "whose matrix values the guard cannot read ({e})"
                    )));
                    vec![UNREAD.to_owned()]
                }
            },
            (Some(_), None) => {
                found.push(unread("read without the job whose matrix gives it"));
                vec![UNREAD.to_owned()]
            }
            (None, _) => {
                found.push(unread("not a matrix value (`${{ matrix.<key> }}`)"));
                vec![UNREAD.to_owned()]
            }
        };
        scripts = scripts
            .iter()
            .flat_map(|script| {
                values
                    .iter()
                    .map(|value| script.replace(text, value))
                    .collect::<Vec<_>>()
            })
            .collect();
    }
    (scripts, found)
}

/// What in a step's script lets a failing command pass the step, each
/// named, empty when the step's status is its commands'.
///
/// A one-line script runs one command, with no shell operator; a longer
/// one starts with `set -euo pipefail` (without `-e` a failing command
/// mid-script is passed over, without `pipefail` a pipeline takes its last
/// command's status, so `nextest | tee` passes a failed run). Neither may
/// have:
/// - an `||` handler but `rc=$?` when the script ends with `exit "$rc"`
///   (or lists a failure from it), a failure-list entry when the script
///   ends by exiting 1 on a non-empty list, or `exit` (`|| true`, `|| :`,
///   `|| echo` and a `[ .. ]` test are rejected by name);
/// - an `exit` whose status may be 0: one with no status but after `||`,
///   or with any but a literal 1 to 255, except `exit "$rc"` as the last
///   line (`exit 0`, `exit "0"`, `exit 00` and `exit $((0))` all fail);
/// - its status variables (`rc`, `failed`, `skips`) set in a form the
///   guard does not read, `rc` read after `rc=0` with no capture since, or
///   the list of failures emptied after it is first filled
///   ([`status_problems`]), or a loop that resets `rc` and can drop a
///   pass's failure ([`loop_problems`]);
/// - a condition that reads no variable, a command substitution anywhere
///   but the value of a plain assignment, a background `&`, or any of
///   [`mask_patterns`];
/// - a `${{ }}` expression but a matrix value of `job`'s, plain words,
///   which is read spliced in ([`spliced`]).
///
/// The one `|| true` allowed is a count's, `$(grep -c .. || true)`, whose
/// exit status 1 means zero.
pub(super) fn shell_masks(step: &str, job: Option<&Hash>) -> Vec<String> {
    let run = match yaml::run_value(step) {
        Ok(Some(run)) => run,
        Ok(None) => return Vec::new(),
        Err(e) => {
            return vec![format!(
                "a step YAML cannot read ({e}), so the guard cannot read its script"
            )];
        }
    };
    let (scripts, mut found) = spliced(&run, job);
    for script in &scripts {
        for mask in script_masks(script) {
            if !found.contains(&mask) {
                found.push(mask);
            }
        }
    }
    found
}

/// What in the script `run`, a `run:` value with its expressions spliced
/// in, lets a failing command pass the step, as [`shell_masks`] names it.
fn script_masks(run: &str) -> Vec<String> {
    let script = script_lines(run);
    let lines: Vec<&str> = script.iter().map(String::as_str).collect();
    let mut found = Vec::new();
    match lines.as_slice() {
        [] => return found,
        [only] => {
            let bare = blank_quotes(only);
            if bare.contains([';', '|', '`']) || bare.contains("$(") || backgrounds(&bare) {
                found.push(format!(
                    "a one-line script that is not one command (`{only}`), so the step's \
                     status may not be the command's"
                ));
            }
        }
        [first, ..] if *first != "set -euo pipefail" => found.push(format!(
            "a script that starts with `{first}`, not `set -euo pipefail`, so a failing \
             command mid-script, or one piped into another, can pass"
        )),
        _ => {}
    }
    found.extend(status_problems(&lines));
    found.extend(loop_problems(&lines));
    let failed_kept = failed_list_exits(&lines);
    let rc_kept = exits_with_rc(&lines) || (lists_failed_rc(&lines) && failed_kept);
    let substitution = regex!(r"\$\((?:[^(]|$)|`|[<>]\(");
    let plain_assignment = regex!(r"^[A-Za-z_]\w*=\$\([^()`]*\)$");
    for (at, line) in lines.iter().enumerate() {
        let blank = blank_quotes(line);
        found.extend(handler_problems(line, &blank, rc_kept, failed_kept));
        found.extend(exit_problems(line, &blank, at + 1 == lines.len()));
        found.extend(condition_problems(line, &blank));
        if substitution.is_match(&blank) && !plain_assignment.is_match(&blank) {
            found.push(format!(
                "a command substitution inside another command, whose status the step \
                 never sees: `{line}`"
            ));
        }
        let bare = grep_count().replace_all(&blank, "$$()");
        let options = option_commands(line, &blank);
        for (pattern, why, reads) in mask_patterns() {
            let hit = match reads {
                Reads::Bare => pattern.is_match(&bare),
                Reads::Options => pattern.is_match(&bare) || pattern.is_match(&options),
                Reads::Line => pattern.is_match(line),
            };
            if hit {
                found.push(format!("{why}: `{line}`"));
            }
        }
        if backgrounds(&bare) {
            found.push(format!(
                "a background `&`, whose command's status the step never sees: `{line}`"
            ));
        }
    }
    found
}

/// The `||` handlers in `line` (`blank` is it with its quotes blanked)
/// that swallow the failure before them, each named. `rc_kept`: the
/// script exits with `rc`, or fails on a failure listed from it;
/// `failed_kept`: it ends by exiting 1 on a non-empty list of failures.
fn handler_problems(line: &str, blank: &str, rc_kept: bool, failed_kept: bool) -> Vec<String> {
    let counts: Vec<Range<usize>> = grep_count().find_iter(blank).map(|m| m.range()).collect();
    let mut found = Vec::new();
    for (at, _) in blank.match_indices("||") {
        if counts.iter().any(|count| count.contains(&at)) {
            continue;
        }
        let handler = line[at + 2..].trim_start();
        let word = blank[at + 2..]
            .trim_start()
            .split(|c: char| c.is_whitespace() || c == ';' || c == ')')
            .next()
            .unwrap_or("");
        let why = match word {
            "true" | ":" => Some("which passes a failing command"),
            "echo" | "printf" => Some("a handler that only prints, so the failure passes"),
            "[" | "[[" | "test" => Some("a test, which passes the failure whenever it holds"),
            // Its status is judged where it stands (`exit_problems`).
            "exit" => None,
            _ if handler == "rc=$?" => {
                (!rc_kept).then_some("which keeps the status, but the script never exits with it")
            }
            _ if handler.starts_with("failed+=(") => (!failed_kept)
                .then_some("but no non-empty list of failures exits 1 at the script's end"),
            _ => Some("a handler the guard does not know to fail the step"),
        };
        if let Some(why) = why {
            found.push(format!("`|| {word}` in `{line}`, {why}"));
        }
    }
    found
}

/// `arg` without one pair of quotes around it.
fn unquoted(arg: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|q| arg.strip_prefix(*q).and_then(|rest| rest.strip_suffix(*q)))
        .unwrap_or(arg)
}

/// The end of the shell word in `blank` that starts at `start`: the first
/// blank, `;`, `|`, `&` or `)` outside the word's own parentheses.
fn word_end(blank: &str, start: usize) -> usize {
    let mut depth = 0usize;
    for (offset, c) in blank[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            c if depth == 0 && (c.is_whitespace() || ";|&)".contains(c)) => return start + offset,
            _ => {}
        }
    }
    blank.len()
}

/// The `exit`s in `line` (`blank` is it with its quotes blanked, `last`
/// whether it is the script's last line) whose status may be 0, each
/// named. Read on the line itself, so a quoted 0 is still a 0.
fn exit_problems(line: &str, blank: &str, last: bool) -> Vec<String> {
    let mut found = Vec::new();
    for exit in regex!(&format!(r"{AT_COMMAND}(exit)\b"))
        .captures_iter(blank)
        .filter_map(|found| found.get(1))
    {
        let rest = &blank[exit.end()..];
        let start = exit.end() + (rest.len() - rest.trim_start().len());
        let arg = &line[start..word_end(blank, start)];
        let after_or = blank[..exit.start()].trim_end().ends_with("||");
        let status = unquoted(arg);
        let non_zero = status.bytes().all(|b| b.is_ascii_digit())
            && status.parse::<u32>().is_ok_and(|n| (1..=255).contains(&n));
        let problem = if arg.is_empty() {
            (!after_or).then(|| {
                format!(
                    "an `exit` with no status, which exits 0 after a command that passed: `{line}`"
                )
            })
        } else if non_zero {
            None
        } else if arg == "\"$rc\"" {
            (!(last && line == "exit \"$rc\"")).then(|| {
                format!(
                    "`exit \"$rc\"` other than as the script's last line, where the guard \
                     cannot tell `rc` holds a status: `{line}`"
                )
            })
        } else {
            Some(format!(
                "`exit {arg}`, whose status may be 0, which passes the step whatever ran \
                 before it: `{line}`"
            ))
        };
        found.extend(problem);
    }
    found
}

/// The `if`, `elif` and loop conditions in `line` (`blank` is it with its
/// quotes blanked) that read no variable, and so are constant: a branch
/// that never runs can hold the step's only exit, or its test run.
fn condition_problems(line: &str, blank: &str) -> Vec<String> {
    regex!(&format!(r"{AT_COMMAND}(?:if|elif|while|until)\s+"))
        .find_iter(blank)
        .filter_map(|found| {
            let end = blank[found.end()..]
                .find(';')
                .map_or(blank.len(), |len| found.end() + len);
            let condition = line[found.end()..end].trim();
            (!condition.contains('$')).then(|| {
                format!(
                    "a condition that reads no variable (`{condition}`), so its branch is \
                     always or never taken: `{line}`"
                )
            })
        })
        .collect()
}

/// What a script does to the variables that carry its verdict beyond the
/// forms the guard reads, each named. `rc`, the status it keeps, is set
/// only by `rc=0`, `|| rc=$?` and `if [ "$rc" -eq 0 ]; then rc=1; fi`, and
/// is not read after an `rc=0` with no capture since. `failed`, the list of
/// failures, is emptied once, at the top level, before its first entry,
/// and grown only by `|| failed+=(..)`. `skips`, the skip markers, is set
/// once, by its count ([`SKIP_COUNT`]).
fn status_problems(lines: &[&str]) -> Vec<String> {
    let shapes = [
        regex!(r"^rc=0$"),
        regex!(r" \|\| rc=\$\?$"),
        regex!(r#"^if \[ "\$rc" -eq 0 \]; then rc=1; fi$"#),
        regex!(r"^failed=\(\)$"),
        regex!(r" \|\| failed\+=\([^()]*\)$"),
        regex!(SKIP_COUNT),
    ];
    let sets = regex!(r"(?:^|[\s;&|({])(rc|failed|skips)(?:\[[^\]]*\])?\+?=");
    let builtin = regex!(&format!(
        r"{AT_COMMAND}(?:let|read|mapfile|readarray|declare|typeset|local|export|readonly|unset|printf)\b[^;&|]*\b(rc|failed|skips)\b"
    ));
    let expansion = regex!(r"\$\{(rc|failed|skips)(?:\[[^\]]*\])?:?=");
    let mut found = Vec::new();
    for line in lines {
        let rest = shapes.iter().fold((*line).to_owned(), |rest, shape| {
            shape.replace(&rest, "").into_owned()
        });
        let blank = blank_quotes(&rest);
        for set in [
            sets.captures(&blank),
            builtin.captures(&blank),
            expansion.captures(&rest),
        ]
        .into_iter()
        .flatten()
        {
            found.push(format!(
                "`{}` set in a form the guard does not read, which can change the status \
                 the step ends with: `{line}`",
                &set[1]
            ));
        }
    }

    let reads_rc = regex!(r"\$\{?rc\b");
    let mut reset = false;
    for line in lines {
        if *line == "rc=0" {
            reset = true;
        } else if line.ends_with(" || rc=$?") {
            reset = false;
        } else if reset && reads_rc.is_match(line) {
            found.push(format!(
                "`$rc` read after `rc=0` with no capture since, so it holds 0, not a \
                 status: `{line}`"
            ));
        }
    }

    let depth = depths(lines);
    let first_entry = lines.iter().position(|line| shapes[4].is_match(line));
    for (at, _) in lines
        .iter()
        .enumerate()
        .filter(|(_, line)| **line == "failed=()")
    {
        if depth[at] > 0
            || first_entry.is_some_and(|entry| at > entry)
            || lines[..at].contains(&"failed=()")
        {
            found.push(format!(
                "`failed=()` on line {} after the first, inside a block, or after the list \
                 is first filled, so it can empty the list before it is read",
                at + 1
            ));
        }
    }
    if lines.iter().filter(|line| shapes[5].is_match(line)).count() > 1 {
        found.push("more than one count of skip markers, so a later one replaces the first".into());
    }
    found
}

/// What may change `rc`: an assignment to it.
const SETS_RC: &str = r"(?:^|[\s;])rc=";

/// The loops in a script that reset `rc` on each pass and can drop a
/// pass's failure before the next pass resets it, each named. A loop that
/// holds an `rc=0`, at any depth, keeps each pass's failure when, at its
/// own top level after the last line in it that sets `rc`, it exits 1 on
/// one (`if [ "$rc" -ne 0 ]; then .. exit 1 .. fi`, or
/// `[ "$rc" -eq 0 ] || exit 1`) or lists it (`[ "$rc" -eq 0 ] ||
/// failed+=(..)`); or when it is a retry, which leaves on the first pass
/// that passes (`if [ "$rc" -eq 0 ]; then break; fi`), so a later pass
/// runs only to replace a failure, and the last one's status stands for
/// what reads `rc` after the loop.
fn loop_problems(lines: &[&str]) -> Vec<String> {
    let depth = depths(lines);
    let mut loops = BTreeSet::new();
    for (at, _) in lines
        .iter()
        .enumerate()
        .filter(|(_, line)| **line == "rc=0")
    {
        // Each block that holds it, innermost first.
        let mut within = depth[at];
        for open in (0..at).rev() {
            if depth[open] < within && opens(lines[open]) {
                within = depth[open];
                if is_loop(lines[open]) {
                    loops.insert(open);
                }
            }
        }
    }
    loops
        .into_iter()
        .filter(|&open| {
            let close = (open + 1..lines.len())
                .find(|&at| depth[at] == depth[open])
                .unwrap_or(lines.len());
            !keeps_each_pass(&lines[open + 1..close])
        })
        .map(|open| {
            format!(
                "a loop that resets `rc` each pass but neither exits on a failed pass, lists \
                 it, nor leaves on a passing one, so a later pass can drop an earlier one's \
                 failure: `{}`",
                lines[open]
            )
        })
        .collect()
}

/// Whether a loop whose body is `body` keeps each pass's failure, as
/// [`loop_problems`] reads it.
fn keeps_each_pass(body: &[&str]) -> bool {
    let depth = depths(body);
    let top = |at: &usize| depth[*at] == 0;
    let retry = (0..body.len()).filter(top).any(|at| {
        body[at] == "if [ \"$rc\" -eq 0 ]; then break; fi"
            || (body[at] == "if [ \"$rc\" -eq 0 ]; then"
                && if_branch(body, at) == ["break"]
                && body.get(at + 2) == Some(&"fi"))
    });
    let after = (0..body.len())
        .rev()
        .find(|&at| regex!(SETS_RC).is_match(body[at]))
        .map_or(0, |at| at + 1);
    let kept = (after..body.len()).filter(top).any(|at| {
        body[at].starts_with("[ \"$rc\" -eq 0 ] || failed+=(")
            || body[at] == "[ \"$rc\" -eq 0 ] || exit 1"
            || (body[at] == "if [ \"$rc\" -ne 0 ]; then"
                && top_level(&if_branch(body, at)).contains(&"exit 1"))
    });
    retry || kept
}

/// Whether `line` opens a loop: a `for`, `while` or `until`.
fn is_loop(line: &str) -> bool {
    ["for ", "while ", "until "]
        .iter()
        .any(|keyword| line.starts_with(keyword))
}

/// Whether `line` opens a block: an `if` or loop not closed on it.
fn opens(line: &str) -> bool {
    (line.starts_with("if ") && !line.ends_with("fi")) || (is_loop(line) && !line.ends_with("done"))
}

/// How deep in `if`s and loops each line sits, a block's `fi` or `done` at
/// the block's own depth.
fn depths(lines: &[&str]) -> Vec<usize> {
    let mut depth = 0usize;
    lines
        .iter()
        .map(|line| {
            if *line == "fi" || *line == "done" {
                depth = depth.saturating_sub(1);
                return depth;
            }
            let here = depth;
            if opens(line) {
                depth += 1;
            }
            here
        })
        .collect()
}

/// The lines of `lines` outside any `if` or loop in it.
fn top_level<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines
        .iter()
        .zip(depths(lines))
        .filter(|(line, depth)| *depth == 0 && **line != "fi" && **line != "done")
        .map(|(line, _)| *line)
        .collect()
}

/// The lines of the `if` branch that starts at `lines[start]`, up to its
/// `elif`, `else` or `fi`, nested `if`s included.
pub(super) fn if_branch<'a>(lines: &[&'a str], start: usize) -> Vec<&'a str> {
    let mut depth = 0;
    let mut branch = Vec::new();
    for line in &lines[start + 1..] {
        let opens = line.starts_with("if ") && !line.ends_with("fi");
        if depth == 0 && (line.starts_with("elif ") || *line == "else" || *line == "fi") {
            break;
        }
        if opens {
            depth += 1;
        } else if *line == "fi" {
            depth -= 1;
        }
        branch.push(*line);
    }
    branch
}

/// Whether a shell line fails the step, or marks it to fail: `exit 1` or
/// `rc=1`, alone or as the body of a one-line `if`.
pub(super) fn fails_the_step(line: &str) -> bool {
    line.split(';')
        .map(|part| part.trim().trim_start_matches("then ").trim())
        .any(|part| part == "exit 1" || part == "rc=1")
}

/// Whether the script ends with `exit "$rc"`.
pub(super) fn exits_with_rc(lines: &[&str]) -> bool {
    lines.last() == Some(&"exit \"$rc\"")
}

/// Whether the script lists a failure from `rc`.
pub(super) fn lists_failed_rc(lines: &[&str]) -> bool {
    lines
        .iter()
        .any(|line| line.starts_with("[ \"$rc\" -eq 0 ] || failed+=("))
}

/// Whether the script ends by exiting 1 on a non-empty list of failures:
/// its last statement is `if [ "${#failed[@]}" -gt 0 ]; then .. fi`, with
/// no `elif` or `else`, and an `exit 1` in its branch, outside any block.
pub(super) fn failed_list_exits(lines: &[&str]) -> bool {
    let Some(at) = lines.iter().rposition(|line| *line == FAILED_CHECK) else {
        return false;
    };
    let branch = if_branch(lines, at);
    let close = at + 1 + branch.len();
    close + 1 == lines.len() && lines[close] == "fi" && top_level(&branch).contains(&"exit 1")
}

/// Whether a step's script pipes a nextest run into another command.
pub(super) fn pipes_nextest(step: &str) -> bool {
    run_script(step).iter().any(|line| {
        let bare = blank_quotes(line);
        bare.contains("nextest") && has_pipe(&bare)
    })
}

/// Each shell-level mask, alone in an otherwise sound step, is named.
#[test]
fn the_shell_mask_check_names_each_mask() {
    let masks = |step: &str| shell_masks(step, None);
    let step = |script: &str| {
        let body: String = script
            .lines()
            .map(|line| format!("          {line}\n"))
            .collect();
        format!(
            "      - name: Guarded\n        run: |\n{body}        env:\n          X: \"a || true\"\n"
        )
    };
    let sound = "set -euo pipefail\nrc=0\ncargo nextest run 2>&1 | tee log || rc=$?\nskips=$(grep -cE 'A|B' log || true)\nif [ \"${skips:-0}\" -gt 3 ]; then\n  echo \"::error::$skips || true\"\n  if [ \"$rc\" -eq 0 ]; then rc=1; fi\nfi\nexit \"$rc\"";
    assert_eq!(masks(&step(sound)), Vec::<String>::new());
    let failed_list = "set -euo pipefail\nfailed=()\nfor t in a b; do\n  rc=0\n  run \"$t\" || rc=$?\n  [ \"$rc\" -eq 0 ] || failed+=(\"$t\")\ndone\nif [ \"${#failed[@]}\" -gt 0 ]; then\n  echo \"::error::failed: ${failed[*]}\"\n  exit 1\nfi";
    assert_eq!(masks(&step(failed_list)), Vec::<String>::new());
    assert!(masks("      - run: cargo check -E 'a | b'\n").is_empty());
    assert!(masks("      - uses: actions/checkout@v4\n").is_empty());
    assert!(
        masks(&step(
            "set -euo pipefail\ncargo test || exit\ncargo test || exit 2"
        ))
        .is_empty()
    );

    for (masked, names) in [
        ("cargo test || true", "|| true"),
        ("cargo test ||true", "|| true"),
        ("cargo test || :", "|| :"),
        ("cargo test || echo failed", "only prints"),
        ("cargo test || touch ok", "does not know"),
        ("cargo test || [ 1 ]", "a test"),
        ("cargo test || [[ 1 ]]", "a test"),
        ("cargo test || [ -n x ]", "a test"),
        ("cargo test || test -s log", "a test"),
        // An exit whose status may be 0, however it is spelled.
        ("cargo test || exit 0", "`exit 0`"),
        ("cargo test\nexit 0", "`exit 0`"),
        ("cargo test || exit \"0\"", "`exit \"0\"`"),
        ("cargo test || exit '0'", "`exit '0'`"),
        ("cargo test || exit 00", "`exit 00`"),
        ("cargo test || exit $((0))", "`exit $((0))`"),
        ("cargo test || exit 256", "`exit 256`"),
        ("cargo test || exit \"$rc\"", "last line"),
        ("cargo test\nexit", "no status"),
        ("cargo test; true", "`true` or `:`"),
        ("cargo test\n:", "`true` or `:`"),
        ("set +e\ncargo test", "set +e"),
        ("set +o pipefail\ncargo test | tee log", "set +e"),
        ("shopt -uo errexit\ncargo test", "shopt"),
        ("shopt -u -o pipefail\ncargo test | tee log", "shopt"),
        ("trap 'exit 0' EXIT\ncargo test", "trap"),
        ("! cargo test", "inverts"),
        ("cargo test && echo ok", "`&&` list"),
        ("if cargo test; then echo ok; fi", "condition"),
        ("while ! cargo test; do sleep 1; done", "condition"),
        ("if [ 1 = 0 ]; then\n  cargo test\nfi", "reads no variable"),
        ("cargo test &", "background"),
        ("exec true", "exec"),
        ("bash -c 'cargo test || true'", "cannot read"),
        ("bash -e -c 'cargo test || true'", "cannot read"),
        ("eval 'cargo test || true'", "cannot read"),
        ("source ./mask.sh\ncargo test", "cannot read"),
        ("run() { cargo test; }\nrun", "function"),
        // P8: an alias stands in for the command the guard reads.
        (
            "shopt -s expand_aliases\nalias cargo=true\ncargo test",
            "`alias`",
        ),
        ("alias cargo=true", "`alias`"),
        ("shopt -s expand_aliases", "`alias`"),
        // ... in POSIX mode, which expands aliases in a script, or set
        // without the word `alias`, its options quoted or not.
        (
            "set -o posix\nBASH_ALIASES[cargo]=true\ncargo test",
            "`BASH_ALIASES`",
        ),
        ("set -o posix", "`alias`"),
        ("set -euo posix", "`alias`"),
        ("set -o \"posix\"", "`alias`"),
        ("shopt -so posix", "`alias`"),
        ("shopt -s 'expand_aliases'", "`alias`"),
        ("declare \"BASH_ALIASES[cargo]=true\"", "`BASH_ALIASES`"),
        // `set +o pipefail` with its option quoted is still read.
        ("set +o \"pipefail\"\ncargo test | tee log", "set +e"),
        ("set '+e'\ncargo test", "set +e"),
        // A loop that resets `rc` each pass and lets a later pass drop an
        // earlier failure: a retry with no exit on its last failure, or a
        // loop that neither exits on a failure nor lists it.
        (
            "for ex in a b; do\n  for attempt in 1 2 3; do\n    rc=0\n    run \"$ex\" || rc=$?\n    if [ \"$rc\" -eq 0 ]; then\n      break\n    fi\n  done\n  if [ \"$rc\" -ne 0 ]; then\n    echo \"::error::$ex failed\"\n  fi\ndone\nexit \"$rc\"",
            "a loop that resets `rc`",
        ),
        (
            "for t in a b; do\n  rc=0\n  run \"$t\" || rc=$?\ndone\nexit \"$rc\"",
            "a loop that resets `rc`",
        ),
        (
            "for t in a b; do\n  rc=0\n  [ \"$rc\" -eq 0 ] || exit 1\n  run \"$t\" || rc=$?\ndone\nexit \"$rc\"",
            "a loop that resets `rc`",
        ),
        // An expression, which the runner splices into the script before
        // bash reads it, wherever it stands, even in a comment.
        ("cargo test ${{ '|| true' }}", "expression"),
        ("cargo test ${{ '||' }} true", "expression"),
        (
            "cargo test \"${{ github.event.pull_request.title }}\"",
            "expression",
        ),
        ("# ${{ '' }}\ncargo test", "expression"),
        ("cargo test ${{ matrix.tests }}", "without the job"),
        ("cargo test ${{ '|| true'", "expression"),
        ("echo \"$(cargo test)\"", "command substitution"),
        ("echo $(cargo test)", "command substitution"),
        ("echo `cargo test`", "command substitution"),
        ("let rc=0", "arithmetic"),
        // `rc` kept, but never exited with, or reset before it is read.
        ("rc=0\ncargo test || rc=$?", "never exits with it"),
        (
            "rc=0\ncargo test || rc=$?\nrc=0\nexit \"$rc\"",
            "read after `rc=0`",
        ),
        (
            "rc=0\ncargo test || rc=$?\nif [ \"$x\" = y ]; then\n  exit \"$rc\"\nfi",
            "never exits with it",
        ),
        (
            "rc=0\ncargo test || rc=$?\nexit \"$rc\"\necho done",
            "last line",
        ),
        (
            "rc=0\ncargo test || rc=$?\nrc=${rc:-0}\nexit \"$rc\"",
            "`rc` set",
        ),
        (
            "rc=0\ncargo test || rc=$?\ndeclare rc=0\nexit \"$rc\"",
            "`rc` set",
        ),
        // The failure list not checked at the end, or emptied first.
        ("cargo test || failed+=(x)", "no non-empty list"),
        (
            "failed=()\ncargo test || failed+=(x)\nfailed=()\nif [ \"${#failed[@]}\" -gt 0 ]; then\n  exit 1\nfi",
            "`failed=()`",
        ),
        (
            "failed=()\ncargo test || failed+=(x)\nif [ \"${#failed[@]}\" -gt 0 ]; then\n  exit 1\nfi\necho done",
            "no non-empty list",
        ),
        (
            "failed=()\ncargo test || failed+=(x)\nif [ \"${#failed[@]}\" -gt 0 ]; then\n  if [ \"$x\" = y ]; then\n    exit 1\n  fi\nfi",
            "no non-empty list",
        ),
        // The skip count replaced.
        ("skips=$(grep -cE 'A' log || true)\nskips=0", "`skips` set"),
        (
            "skips=$(grep -cE 'A' log || true)\nskips=$(grep -cE 'A' other || true)",
            "more than one count",
        ),
    ] {
        let found = masks(&step(&format!("set -euo pipefail\n{masked}")));
        assert!(
            found.iter().any(|escape| escape.contains(names)),
            "`{masked}`: want a mask naming `{names}`, found {found:?}"
        );
    }
    // A retry loop inside a loop that exits on an example's last failure,
    // or lists it, keeps every pass's failure.
    for kept in [
        "for ex in a b; do\n  for attempt in 1 2 3; do\n    rc=0\n    run \"$ex\" || rc=$?\n    if [ \"$rc\" -eq 0 ]; then\n      break\n    fi\n  done\n  if [ \"$rc\" -ne 0 ]; then\n    echo \"::error::$ex failed\"\n    exit 1\n  fi\ndone\nexit \"$rc\"",
        "for t in a b; do\n  rc=0\n  run \"$t\" || rc=$?\n  [ \"$rc\" -eq 0 ] || exit 1\ndone\nexit \"$rc\"",
        "for attempt in 1 2 3; do\n  rc=0\n  run || rc=$?\n  if [ \"$rc\" -eq 0 ]; then break; fi\ndone\nexit \"$rc\"",
    ] {
        let found = masks(&step(&format!("set -euo pipefail\n{kept}")));
        assert_eq!(found, Vec::<String>::new(), "{kept}");
    }
    // A matrix value is read spliced in, one script for each, and must be
    // plain words; a matrix the guard cannot read is named.
    let matrix_step =
        step("set -euo pipefail\nfor t in ${{ matrix.tests }}; do\n  run \"$t\"\ndone");
    let job = |matrix: &str| {
        yaml::job_mapping(&format!("    strategy:\n      matrix:\n{matrix}"))
            .expect("a job mapping")
    };
    let sound = job("        include:\n          - tests: \"a b\"\n          - tests: c.d/e\n");
    assert_eq!(
        shell_masks(&matrix_step, Some(&sound)),
        Vec::<String>::new()
    );
    let one_line = "      - run: run ${{ matrix.cmd }}\n";
    assert!(shell_masks(one_line, Some(&job("        cmd: [a, b]\n"))).is_empty());
    for (matrix, names) in [
        (
            "        include:\n          - tests: \"a; true\"\n",
            "plain words",
        ),
        ("        tests: [a, \"b || true\"]\n", "plain words"),
        ("        tests: [a, [b]]\n", "not a scalar"),
        ("        other: [a]\n", "no value"),
        (
            "        include:\n          - <<: {tests: \"a || true\"}\n",
            "merge key",
        ),
    ] {
        let found = shell_masks(&matrix_step, Some(&job(matrix)));
        assert!(
            found.iter().any(|escape| escape.contains(names)),
            "{matrix}: want a mask naming `{names}`, found {found:?}"
        );
    }
    let from_json =
        yaml::job_mapping("    strategy:\n      matrix: ${{ fromJSON(needs.a.outputs.m) }}\n")
            .expect("a job mapping");
    assert!(
        shell_masks(&matrix_step, Some(&from_json))
            .iter()
            .any(|escape| escape.contains("not a mapping"))
    );
    // A value spliced into the command's own place is read there: a `true`
    // run for the step's command.
    let found = shell_masks(
        "      - run: ${{ matrix.cmd }}\n",
        Some(&job("        cmd: [cargo, \"true\"]\n")),
    );
    assert!(
        found.iter().any(|escape| escape.contains("`true` or `:`")),
        "{found:?}"
    );

    // A script without the flags, and one-line scripts that are not one
    // command.
    assert!(!masks(&step("set -eo pipefail\ncargo test")).is_empty());
    assert!(!masks(&step("cargo test\ncargo doc")).is_empty());
    for one_line in [
        "cargo test || true",
        "cargo test; true",
        "cargo test | tee log",
        "cargo test &",
        "echo $(cargo test)",
        "cargo test || exit \"0\"",
    ] {
        assert!(
            !masks(&format!("      - name: Guarded\n        run: {one_line}\n")).is_empty(),
            "{one_line}"
        );
    }
}

#[test]
fn the_run_script_reader_reads_what_the_shell_runs() {
    let masks = |step: &str| shell_masks(step, None);
    let step = "      - name: Live\n        if: always()\n        run: |\n          set -euo pipefail\n          # a comment\n\n          cargo nextest run \\\n            --run-ignored all | tee log\n        env:\n          X: \"run: no\"\n";
    assert_eq!(
        run_script(step),
        [
            "set -euo pipefail",
            "cargo nextest run --run-ignored all | tee log"
        ]
    );
    assert_eq!(
        run_script("      - run: cargo check\n        env:\n          A: b\n"),
        ["cargo check"]
    );
    assert_eq!(
        run_script("      - name: Q\n        run: 'it''s'\n"),
        ["it's"]
    );
    // What YAML joins, the reader joins: a plain value's deeper
    // continuation line (P1, P3), a folded block, a quoted value over two
    // lines. The shell runs each as one command.
    for (step, script) in [
        (
            "      - name: Floor\n        run: cargo llvm-cov report --fail-under-lines 93\n          || true\n        env:\n          A: b\n",
            "cargo llvm-cov report --fail-under-lines 93 || true",
        ),
        (
            "      - run: >\n          cargo nextest run\n          || true\n",
            "cargo nextest run || true",
        ),
        (
            "      - run: \"cargo nextest run\n          || true\"\n",
            "cargo nextest run || true",
        ),
    ] {
        assert_eq!(run_script(step), [script], "{step}");
        assert!(!masks(step).is_empty(), "{step}");
    }
    assert!(
        run_script("      - uses: actions/checkout@v4\n        with:\n          run: x\n")
            .is_empty()
    );
    // Blanked byte for byte, so an offset in one is the same in the other;
    // a command substitution in double quotes stays visible.
    for (line, blanked) in [
        (
            r#"echo "a | b \" c" 'd || e' f\;g"#,
            r#"echo "__________" '______' f__g"#,
        ),
        ("echo \"— x\" ok", "echo \"_____\" ok"),
        (
            "echo \"$(a)\" '$(b)' \"$((1))\"",
            "echo \"$(__\" '____' \"______\"",
        ),
        ("echo \"`a`\"", "echo \"`_`\""),
    ] {
        assert_eq!(blank_quotes(line), blanked, "{line}");
        assert_eq!(blank_quotes(line).len(), line.len(), "{line}");
    }
    assert!(pipes_nextest(step));
    assert!(!pipes_nextest(
        "      - run: echo \"cargo nextest run -E 'a | b'\"\n"
    ));
}
