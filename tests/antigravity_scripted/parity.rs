//! The Python SDK parity guard: every harness option the installed
//! `google-antigravity` wheel exposes is in the parity table of
//! `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`, modeled or with the reason it is
//! not, and the table names nothing the wheel lacks. The wheel's sources are
//! read with `ast` by `.github/scripts/dump_sdk_options.py`, never imported.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The design doc, read at test time so an edit needs no rebuild.
fn design_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/ANTIGRAVITY_BRIDGE_DESIGN.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The parity section's table rows: each line of it that starts with a
/// backticked key.
fn parity_rows(doc: &str) -> Vec<&str> {
    let start = doc
        .find("## Python SDK parity")
        .expect("the design doc has a Python SDK parity section");
    let section = &doc[start..];
    let end = section[3..].find("\n## ").map_or(section.len(), |i| i + 3);
    section[..end]
        .lines()
        .filter(|line| line.starts_with("| `"))
        .collect()
}

/// A table row's cells, trimmed, split on the pipes Markdown does not
/// escape: `add_custom_trigger(\|ctx\| ..)` is one cell.
fn cells(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = row.trim().trim_start_matches('|').chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_string()),
            c => cell.push(c),
        }
    }
    if !cell.trim().is_empty() {
        cells.push(cell.trim().to_string());
    }
    cells
}

/// The keys in the first column of the parity section's tables: every
/// backticked token in it.
fn table_keys(doc: &str) -> BTreeSet<String> {
    parity_rows(doc)
        .into_iter()
        .filter_map(|row| cells(row).into_iter().next())
        .flat_map(|cell| {
            cell.split('`')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The rows that break the table's promise, each with what is missing:
/// every key maps to something in the genai-rs column, and a row that is
/// not modeled gives its reason in the notes column. That is a row whose
/// genai-rs cell says "Not modeled" (even beside a backticked name), or
/// names no API at all (no backticked name: "N/A", whatever its wording).
fn row_problems(doc: &str) -> Vec<String> {
    parity_rows(doc)
        .into_iter()
        .filter_map(|row| {
            let cells = cells(row);
            let mapping = cells.get(1).map_or("", String::as_str);
            let notes = cells.get(2).map_or("", String::as_str);
            if mapping.is_empty() {
                Some(format!("no genai-rs mapping: {row}"))
            } else if (mapping.starts_with("Not modeled") || !mapping.contains('`'))
                && notes.is_empty()
            {
                Some(format!("not modeled, with no reason in its notes: {row}"))
            } else {
                None
            }
        })
        .collect()
}

/// The site-packages directory holding the wheel under test: beside
/// `ANTIGRAVITY_HARNESS_PATH`, in python3's own site-packages, or beside a
/// `localharness` on `PATH`.
fn wheel_site_packages() -> Option<PathBuf> {
    // .../<site-packages>/google/antigravity/bin/localharness
    let from_binary = |binary: PathBuf| {
        let binary = binary.canonicalize().unwrap_or(binary);
        binary
            .ancestors()
            .nth(4)
            .map(Path::to_path_buf)
            .filter(|dir| dir.join("google/antigravity/types.py").is_file())
    };
    // As `spawn()` takes it: a non-empty variable is the harness, and an
    // empty one is unset.
    if let Some(path) = std::env::var_os("ANTIGRAVITY_HARNESS_PATH").filter(|p| !p.is_empty()) {
        return from_binary(PathBuf::from(path));
    }
    let site = std::process::Command::new("python3")
        .args([
            "-I",
            "-c",
            "import site\nprint('\\n'.join(site.getsitepackages() + [site.getusersitepackages()]))",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(PathBuf::from)
                .find(|dir| dir.join("google/antigravity/types.py").is_file())
        });
    site.or_else(|| {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("localharness"))
            .find(|candidate| candidate.is_file())
            .and_then(from_binary)
    })
}

#[test]
fn table_keys_reads_the_first_column_only() {
    let doc = "## Other\n| `Not.this` | x |\n## Python SDK parity (x)\n\n\
               | Option | genai-rs |\n|---|---|\n\
               | `A.one`, `A.two` | `with_one` |\n| `B.three` | Not modeled |\n\
               ## Next\n| `C.four` | y |\n";
    let keys: Vec<_> = table_keys(doc).into_iter().collect();
    assert_eq!(keys, ["A.one", "A.two", "B.three"]);
}

#[test]
fn row_problems_finds_a_row_with_no_mapping_or_no_reason() {
    let doc = "## Python SDK parity (x)\n\n\
               | Option | genai-rs | Notes |\n|---|---|---|\n\
               | `A.mapped` | `with_a` | |\n\
               | `B.skipped` | Not modeled | The reason. |\n\
               | `C.piped` | `add(\\|x\\| x)` | |\n\
               | `D.bare` | Not modeled | |\n\
               | `E.blank` |  | A note. |\n\
               | `F.short` | Not modeled (preset) |\n\
               | `H.na` | N/A | |\n\
               | `I.unsupported` | Unsupported |   |\n\
               | `J.reasoned` | Unsupported | The reason. |\n\
               | `K.partial` | Not modeled; use `with_k` | |\n\
               ## Next\n| `G.other` | Not modeled | |\n";
    let problems = row_problems(doc);
    assert_eq!(problems.len(), 6, "{problems:#?}");
    assert!(problems[0].contains("`D.bare`") && problems[0].contains("no reason"));
    assert!(problems[1].contains("`E.blank`") && problems[1].contains("no genai-rs mapping"));
    assert!(problems[2].contains("`F.short`") && problems[2].contains("no reason"));
    // Any wording that names no API needs a reason, not only "Not modeled".
    assert!(problems[3].contains("`H.na`") && problems[3].contains("no reason"));
    assert!(problems[4].contains("`I.unsupported`") && problems[4].contains("no reason"));
    // "Not modeled" needs a reason even when the cell names an API too.
    assert!(problems[5].contains("`K.partial`") && problems[5].contains("no reason"));
    assert_eq!(
        cells("| `C.piped` | `add(\\|x\\| x)` | |")[1],
        "`add(|x| x)`"
    );
}

/// Key-free: the table itself, whatever wheel is installed.
#[test]
fn every_parity_row_maps_its_keys_and_gives_a_reason_when_not_modeled() {
    let problems = row_problems(&design_doc());
    assert!(
        problems.is_empty(),
        "the parity table in docs/ANTIGRAVITY_BRIDGE_DESIGN.md has rows that map nothing, or \
         are not modeled with no reason in their Notes cell:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
#[ignore = "Requires localharness binary"]
fn test_python_sdk_options_are_all_in_the_parity_table() {
    let site = wheel_site_packages().expect(
        "no google-antigravity wheel found (ANTIGRAVITY_HARNESS_PATH, python3 site-packages, \
         PATH): the parity guard checked nothing",
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/scripts/dump_sdk_options.py");
    let output = std::process::Command::new("python3")
        .arg("-I")
        .arg(&script)
        .arg(&site)
        .output()
        .expect("run python3");
    assert!(
        output.status.success(),
        "dump_sdk_options.py failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sdk: BTreeSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    let table = table_keys(&design_doc());

    let unlisted: Vec<_> = sdk.difference(&table).collect();
    let stale: Vec<_> = table.difference(&sdk).collect();
    assert!(
        unlisted.is_empty() && stale.is_empty(),
        "the parity table in docs/ANTIGRAVITY_BRIDGE_DESIGN.md is out of step with the wheel at \
         {}.\n  Options the wheel exposes that the table does not list (model each, or list it \
         with the reason it is not modeled): {unlisted:?}\n  Table keys the wheel no longer \
         has: {stale:?}",
        site.display()
    );
}
