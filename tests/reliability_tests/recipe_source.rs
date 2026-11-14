//! Where the retry recipes come from (`retry_recipes`): the fenced code
//! blocks of every guide, README and doc comment, and each example's whole
//! file, read as code with comments, string text and doctest-hidden lines
//! dropped.

use crate::source_scan;

/// One fenced code block in a doc: where it starts, and its lines (a doc
/// comment's marker stripped).
pub(crate) struct DocBlock {
    pub(crate) at: String,
    pub(crate) lines: Vec<String>,
}

/// The doc text a line of Rust holds, in any of rustdoc's forms: a `///`
/// or `//!` line, a line of a `/** */` or `/*! */` block (`in_block`
/// carries an open one to the next line), or a one-line
/// `#[doc = "..."]` or `#![doc = "..."]` string, plain or raw, whose `\n`
/// escapes split it into lines. `None` when the line holds none.
pub(crate) fn doc_text(line: &str, in_block: &mut bool) -> Option<Vec<String>> {
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
pub(crate) fn doc_blocks(path: &str, text: &str) -> Vec<DocBlock> {
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
pub(crate) fn code_lines(lines: &[String]) -> Vec<String> {
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

/// Every Markdown file at the top level, and every Markdown and `.rs` file
/// under `docs/`, `examples/` and the library crates' sources:
/// repository-relative path and text.
pub(crate) fn doc_sources() -> Vec<(String, String)> {
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
pub(crate) fn scanned_blocks(path: &str, text: &str) -> Vec<DocBlock> {
    if path.starts_with("examples/") && path.ends_with(".rs") {
        vec![DocBlock {
            at: format!("{path}:1"),
            lines: text.lines().map(str::to_string).collect(),
        }]
    } else {
        doc_blocks(path, text)
    }
}

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
