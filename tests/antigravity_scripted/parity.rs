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

/// The keys in the first column of the parity section's tables: every
/// backticked token in it.
fn table_keys(doc: &str) -> BTreeSet<String> {
    let start = doc
        .find("## Python SDK parity")
        .expect("the design doc has a Python SDK parity section");
    let section = &doc[start..];
    let end = section[3..].find("\n## ").map_or(section.len(), |i| i + 3);
    section[..end]
        .lines()
        .filter(|line| line.starts_with("| `"))
        .filter_map(|line| line.split('|').nth(1))
        .flat_map(|cell| {
            cell.split('`')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect::<Vec<_>>()
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
    if let Some(path) = std::env::var_os("ANTIGRAVITY_HARNESS_PATH") {
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
