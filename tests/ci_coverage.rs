//! Guards the link between live tests, the CI integration matrix and the
//! API surface.
//!
//! `rust.yml`'s `test-integration` job runs `--run-ignored all` per binary
//! listed in its matrix groups, and `release.yml` is the only other place
//! live tests run — after the tag exists. A live test binary left out of the
//! matrix therefore goes unexercised on every PR, which is how
//! `streaming_resume_tests` and `tool_service_tests` sat unrun. Likewise
//! `fuzz.yml`'s matrix runs every cargo-fuzz target in `fuzz/Cargo.toml`,
//! and its `Fuzz Gate` job reports on every pull request, so branch
//! protection can require it.
//!
//! The second half ties the committed API-surface snapshot
//! (`tests/fixtures/api_surface/`) to the crate: every endpoint maps to the
//! handle verbs that call it, every verb of every resource handle is called
//! by a live test on that handle (`tests/live_verbs/`), and every copy of
//! the `Api-Revision` value is the one the bindings pin.
//!
//! The workflows themselves are guarded in `tests/ci_workflows/`: no nextest
//! run fails fast, retries are set in `NEXTEST_RETRIES` where the flakiness
//! report sees them, trybuild's `ui_tests` stay off beta, every scheduled
//! workflow and every push to main escalates its failures, a PR runs the
//! Windows leg, the flakiness report reads every retry it claims to count,
//! and Code Coverage writes its summary on every run.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test reports a failure by panicking"
)]

mod ci_workflows;
#[path = "api_surface/claims.rs"]
mod claims;
mod live_verbs;

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Run in the `test-antigravity` job's own steps, not the matrix (see
/// `outside_matrix_binaries_run_in_the_antigravity_job`).
const OUTSIDE_MATRIX: &[&str] = &["antigravity_harness", "antigravity_scripted"];

/// The one ignore reason for a live test (CLAUDE.md, "Tests").
const LIVE_REASON: &str = "Requires API key";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `(binary name, source)` for every top-level test file.
fn test_sources() -> Vec<(String, String)> {
    let dir = root().join("tests");
    let mut sources: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).expect("read test source");
            (name, text)
        })
        .collect();
    sources.sort();
    assert!(
        !sources.is_empty(),
        "found no test sources in {}",
        dir.display()
    );
    sources
}

/// Binaries named in the matrix's `tests: "…"` lines.
fn matrix_binaries() -> Vec<String> {
    let workflow = rust_yml();
    let job = workflow
        .split("\n  test-integration:")
        .nth(1)
        .expect("rust.yml has no test-integration job");
    let job = job.split("\n  fmt:").next().unwrap();
    let binaries: Vec<String> = job
        .lines()
        .filter_map(|line| line.trim().strip_prefix("tests: \""))
        .flat_map(|list| list.trim_end_matches('"').split_whitespace())
        .map(str::to_string)
        .collect();
    assert!(
        !binaries.is_empty(),
        "no matrix groups found in test-integration"
    );
    binaries
}

fn ignore_reasons(source: &str) -> impl Iterator<Item = &str> {
    source.lines().filter_map(|line| {
        line.trim()
            .strip_prefix("#[ignore = \"")
            .and_then(|rest| rest.strip_suffix("\"]"))
    })
}

#[test]
fn every_live_test_binary_is_in_the_ci_matrix() {
    let matrix = matrix_binaries();
    let unique: BTreeSet<&String> = matrix.iter().collect();
    assert_eq!(
        unique.len(),
        matrix.len(),
        "a binary is listed twice: {matrix:?}"
    );

    let sources = test_sources();
    let names: BTreeSet<&str> = sources.iter().map(|(name, _)| name.as_str()).collect();
    for binary in &matrix {
        assert!(
            names.contains(binary.as_str()),
            "the matrix lists {binary}, but tests/{binary}.rs does not exist"
        );
    }

    let missing: Vec<&str> = sources
        .iter()
        .filter(|(name, text)| {
            !OUTSIDE_MATRIX.contains(&name.as_str())
                && ignore_reasons(text).next().is_some()
                && !matrix.contains(name)
        })
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "these binaries have live tests but no test-integration matrix group: {missing:?}"
    );
}

/// The binaries kept out of the matrix still have to run somewhere: each is
/// named by a `--run-ignored all` filter in the `test-antigravity` job.
#[test]
fn outside_matrix_binaries_run_in_the_antigravity_job() {
    let workflow = rust_yml();
    let job = workflow
        .split("\n  test-antigravity:\n")
        .nth(1)
        .expect("rust.yml has no test-antigravity job")
        .split("\n  test-integration:")
        .next()
        .unwrap();
    for binary in OUTSIDE_MATRIX {
        let runs = job.lines().any(|line| {
            line.contains("--run-ignored all") && line.contains(&format!("binary({binary})"))
        });
        assert!(
            runs,
            "{binary} is kept out of the matrix but no test-antigravity step runs it"
        );
    }
}

#[test]
fn live_tests_use_the_standard_ignore_reason() {
    let mut offenders = Vec::new();
    for (name, text) in test_sources() {
        if OUTSIDE_MATRIX.contains(&name.as_str()) {
            continue;
        }
        for reason in ignore_reasons(&text) {
            if reason != LIVE_REASON {
                offenders.push(format!("tests/{name}.rs: {reason:?}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "use #[ignore = \"{LIVE_REASON}\"] for live tests:\n  {}",
        offenders.join("\n  ")
    );
}

/// `rust.yml` with line endings normalized: a Windows checkout has CRLF
/// (autocrlf), and the parsers here split on `\n`-delimited headers.
fn rust_yml() -> String {
    std::fs::read_to_string(root().join(".github/workflows/rust.yml"))
        .expect("read rust.yml")
        .replace("\r\n", "\n")
}

/// Job ids under `jobs:` in `rust.yml`: its two-space-indented keys.
fn rust_yml_jobs(workflow: &str) -> BTreeSet<String> {
    let jobs = workflow
        .split("\njobs:\n")
        .nth(1)
        .expect("rust.yml has no jobs: section");
    jobs.lines()
        .filter(|line| line.starts_with("  ") && !line.starts_with("   "))
        .filter_map(|line| line.trim().strip_suffix(':'))
        .map(str::to_string)
        .collect()
}

/// `scheduled-run-status` escalates a red weekly run by reading
/// `needs.*.result`, so a job missing from its `needs` could fail on the
/// schedule without opening the escalation issue, and the resolve step would
/// even close an open one.
#[test]
fn scheduled_run_status_needs_every_other_job() {
    let workflow = rust_yml();
    let job = workflow
        .split("\n  scheduled-run-status:\n")
        .nth(1)
        .expect("rust.yml has no scheduled-run-status job");
    let needs: BTreeSet<String> = job
        .lines()
        .find_map(|line| line.trim().strip_prefix("needs: ["))
        .expect("scheduled-run-status has no one-line `needs: [...]`")
        .trim_end_matches(']')
        .split(',')
        .map(|job| job.trim().to_string())
        .collect();
    let mut expected = rust_yml_jobs(&workflow);
    assert!(expected.len() > 5, "parsed too few jobs: {expected:?}");
    assert!(expected.remove("scheduled-run-status"));
    assert_eq!(
        needs, expected,
        "scheduled-run-status must need every other rust.yml job"
    );
}

// =============================================================================
// The fuzz workflow
// =============================================================================

/// `fuzz.yml`, line endings normalized as for [`rust_yml`].
fn fuzz_yml() -> String {
    std::fs::read_to_string(root().join(".github/workflows/fuzz.yml"))
        .expect("read fuzz.yml")
        .replace("\r\n", "\n")
}

/// The `name = "..."` of every `[[bin]]` in `fuzz/Cargo.toml`: the targets
/// `cargo fuzz` can run.
fn fuzz_bins() -> BTreeSet<String> {
    let manifest = std::fs::read_to_string(root().join("fuzz/Cargo.toml"))
        .expect("read fuzz/Cargo.toml")
        .replace("\r\n", "\n");
    let mut bins = BTreeSet::new();
    let mut in_bin = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_bin = line == "[[bin]]";
        } else if in_bin && let Some(name) = line.strip_prefix("name = \"") {
            bins.insert(name.trim_end_matches('"').to_string());
        }
    }
    assert!(!bins.is_empty(), "no [[bin]] found in fuzz/Cargo.toml");
    bins
}

/// A fuzz target missing from `fuzz.yml`'s matrix never runs in CI, and a
/// matrix entry with no `[[bin]]` fails every run. Each target also needs
/// committed seeds, which the required Test Suite replays
/// (`committed_seeds_replay_through_their_targets` in
/// `src/fuzzing_tests.rs`).
#[test]
fn fuzz_matrix_runs_every_fuzz_target() {
    let workflow = fuzz_yml();
    let matrix: Vec<String> = workflow
        .lines()
        .find_map(|line| line.trim().strip_prefix("target: ["))
        .expect("fuzz.yml has no one-line `target: [...]` matrix")
        .trim_end_matches(']')
        .split(',')
        .map(|target| target.trim().to_string())
        .collect();
    let unique: BTreeSet<String> = matrix.iter().cloned().collect();
    assert_eq!(
        unique.len(),
        matrix.len(),
        "a target is listed twice: {matrix:?}"
    );
    let bins = fuzz_bins();
    assert_eq!(
        unique, bins,
        "fuzz.yml's target matrix must list every [[bin]] in fuzz/Cargo.toml, and nothing else"
    );
    for bin in &bins {
        let seeds = root().join("fuzz/seeds").join(bin);
        assert!(
            seeds.is_dir(),
            "the {bin} target has no fuzz/seeds/{bin}/ directory"
        );
    }
}

/// The matrix's checks exist only on runs that fuzz, so branch protection
/// can require only a job that reports on every pull request: `Fuzz Gate`.
/// A `paths:` filter on the trigger would stop the whole workflow, leaving
/// that required check pending forever, so the path check is a job.
#[test]
fn fuzz_gate_reports_on_every_pull_request() {
    let workflow = fuzz_yml();
    let (on, jobs) = workflow
        .split_once("\njobs:\n")
        .expect("fuzz.yml has no jobs: section");
    assert!(
        on.lines().any(|line| line.trim() == "pull_request:"),
        "fuzz.yml no longer runs on pull requests"
    );
    let filtered: Vec<&str> = on
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("paths:") || line.starts_with("paths-ignore:"))
        .collect();
    assert!(
        filtered.is_empty(),
        "fuzz.yml's triggers are path-filtered ({filtered:?}): a pull request that \
         skips the workflow never reports the Fuzz Gate check"
    );

    let gate = jobs
        .split("\n  fuzz-gate:\n")
        .nth(1)
        .expect("fuzz.yml has no fuzz-gate job")
        .split("\n  scheduled-run-status:\n")
        .next()
        .unwrap();
    let field = |key: &str| {
        gate.lines()
            .find_map(|line| line.strip_prefix("    ")?.strip_prefix(key))
            .map(str::trim)
            .unwrap_or_else(|| panic!("the fuzz-gate job has no `{key}` line"))
    };
    assert_eq!(field("name:"), "Fuzz Gate", "branch protection names it");
    assert_eq!(field("if:"), "always()", "it must report when fuzz fails");
    let needs: BTreeSet<&str> = field("needs:")
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(str::trim)
        .collect();
    assert!(
        needs.contains("fuzz") && needs.contains("changes"),
        "the gate must need the path check and the fuzz matrix: {needs:?}"
    );
}

// =============================================================================
// The API-surface snapshot and the resource handles
// =============================================================================

/// `tests/fixtures/api_surface/<file>`, parsed.
fn snapshot(file: &str) -> serde_json::Value {
    let path = root().join("tests/fixtures/api_surface").join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

/// The coverage oracle reads `bindings.json`; `api-surface-sweep` diffs
/// against `.github/last-swept-sdk-version`. They must name the same
/// google-genai release, or the oracle tests surface the sweep never saw.
/// (The sweep, and the `shell-scripts` job when the dump script changes,
/// check the content too.)
#[test]
fn api_snapshot_matches_the_sweep_baseline() {
    let baseline = std::fs::read_to_string(root().join(".github/last-swept-sdk-version"))
        .expect("read .github/last-swept-sdk-version");
    let bindings = snapshot("bindings.json");
    assert_eq!(
        bindings["sdk_version"].as_str(),
        Some(baseline.trim()),
        "tests/fixtures/api_surface/bindings.json is not for the sweep baseline \
         google-genai {}: regenerate it with .github/scripts/refresh_api_snapshot.sh",
        baseline.trim()
    );
}

/// Every copy of the `Api-Revision` value, as `(file, the start of its line
/// up to the quoted value)`: the crate's, the scripts' that call the API
/// directly, and the resource stub tests' that expect it on the wire.
const API_REVISION_COPIES: &[(&str, &str)] = &[
    (
        "src/http/common.rs",
        "pub(crate) const API_REVISION: &str = ",
    ),
    (
        ".github/scripts/cleanup_test_resources.py",
        "API_REVISION = ",
    ),
    (".github/scripts/capture_fixtures.py", "API_REVISION = "),
    (
        "tests/http_mock_resources.rs",
        "const API_REVISION: &str = ",
    ),
];

/// The quoted values of the lines of `source` that start with `prefix`.
fn quoted_constants(source: &str, prefix: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix(prefix))
        .map(|rest| {
            rest.trim()
                .trim_end_matches(';')
                .trim_matches('"')
                .to_string()
        })
        .collect()
}

/// The crate sends the `Api-Revision` the bindings pin
/// (`bindings.json`'s `api_revision`, google-genai's
/// `GOOGLE_GENAI_API_REVISION`), and so does every other copy. A baseline
/// bump that moves the revision fails here until each copy moves with it,
/// after the new revision's wire changes are swept.
#[test]
fn api_revision_copies_match_the_bindings() {
    let bindings = snapshot("bindings.json");
    let pinned = bindings["api_revision"].as_str().expect(
        "bindings.json has no api_revision: regenerate it with \
         .github/scripts/refresh_api_snapshot.sh",
    );
    let mut problems = Vec::new();
    for (file, prefix) in API_REVISION_COPIES {
        let source = std::fs::read_to_string(root().join(file))
            .unwrap_or_else(|e| panic!("cannot read {file}: {e}"))
            .replace("\r\n", "\n");
        match quoted_constants(&source, prefix).as_slice() {
            [value] if value == pinned => {}
            [value] => problems.push(format!(
                "{file}: {value:?}, but google-genai {} pins {pinned:?}",
                bindings["sdk_version"]
            )),
            found => problems.push(format!(
                "{file}: expected one `{prefix}\"...\"` line, found {found:?}"
            )),
        }
    }
    assert!(
        problems.is_empty(),
        "Api-Revision copies that differ from the bindings':\n  {}",
        problems.join("\n  ")
    );
}

/// A copy is read from its own line only: not from a longer name that
/// starts the same (`API_REVISION_HEADER`), nor from a comment.
#[test]
fn an_api_revision_copy_is_read_from_its_own_line() {
    let source = "pub(crate) const API_REVISION_HEADER: &str = \"Api-Revision\";\n\
                  /// Sent as `API_REVISION = \"x\"`.\n\
                  pub(crate) const API_REVISION: &str = \"2026-05-20\";\n";
    assert_eq!(
        quoted_constants(source, "pub(crate) const API_REVISION: &str = "),
        ["2026-05-20"]
    );
    assert_eq!(
        quoted_constants(
            "GOOGLE_API_REVISION = \"a\"\nAPI_REVISION = \"b\"\n",
            "API_REVISION = "
        ),
        ["b"]
    );
}

/// Every endpoint in the snapshot, and the handle verbs that call it. The
/// Files, File Search Store, Document and Operation endpoints come from
/// google-genai's hand-written modules, which name a resource by its full
/// name; the dump spells those out (`fileSearchStores/{}/documents/{}`)
/// and keeps a name it cannot place (`{operationName}`).
const ENDPOINTS: &[(&str, &[&str])] = &[
    ("DELETE /{}/agents/{}", &["Agents::delete"]),
    ("DELETE /{}/credentials/{}", &["Credentials::delete"]),
    ("DELETE /{}/environments/{}", &["Environments::delete"]),
    (
        "DELETE /{}/fileSearchStores/{}",
        &["FileSearchStores::delete", "FileSearchStores::force_delete"],
    ),
    (
        "DELETE /{}/fileSearchStores/{}/documents/{}",
        &[
            "FileSearchDocuments::delete",
            "FileSearchDocuments::force_delete",
        ],
    ),
    ("DELETE /{}/files/{}", &["Files::delete"]),
    ("DELETE /{}/interactions/{}", &["Interactions::delete"]),
    ("DELETE /{}/triggers/{}", &["Triggers::delete"]),
    ("DELETE /{}/voices/{}", &["Voices::delete"]),
    ("DELETE /{}/webhooks/{}", &["Webhooks::delete"]),
    ("GET /{}/agents", &["Agents::list"]),
    ("GET /{}/agents/{}", &["Agents::get"]),
    ("GET /{}/credentials", &["Credentials::list"]),
    ("GET /{}/credentials/{}", &["Credentials::get"]),
    ("GET /{}/environments", &["Environments::list"]),
    ("GET /{}/environments/{}", &["Environments::get"]),
    // One path: a listing, or with `alt=media` the file itself.
    (
        "GET /{}/environments/{}/files/{}",
        &["EnvironmentFiles::list", "EnvironmentFiles::download"],
    ),
    ("GET /{}/fileSearchStores", &["FileSearchStores::list"]),
    ("GET /{}/fileSearchStores/{}", &["FileSearchStores::get"]),
    (
        "GET /{}/fileSearchStores/{}/documents",
        &["FileSearchDocuments::list"],
    ),
    // An upload and an import read the document they made back.
    (
        "GET /{}/fileSearchStores/{}/documents/{}",
        &[
            "FileSearchDocuments::get",
            "FileSearchDocuments::wait_until_active",
            "FileSearchStores::upload",
            "FileSearchStores::import_file",
        ],
    ),
    ("GET /{}/files", &["Files::list"]),
    (
        "GET /{}/files/{}",
        &["Files::get", "Files::wait_until_active"],
    ),
    (
        "GET /{}/interactions/{}",
        &[
            "Interactions::get",
            "Interactions::get_with_input",
            "Interactions::stream",
            "Interactions::resume_stream",
        ],
    ),
    ("GET /{}/triggers", &["Triggers::list"]),
    ("GET /{}/triggers/{}", &["Triggers::get"]),
    (
        "GET /{}/triggers/{}/executions",
        &["Triggers::list_executions"],
    ),
    ("GET /{}/voices", &["Voices::list"]),
    ("GET /{}/voices/{}", &["Voices::get"]),
    ("GET /{}/webhooks", &["Webhooks::list"]),
    ("GET /{}/webhooks/{}", &["Webhooks::get"]),
    // An operation by its full name: an import polls the one it starts
    // (`fileSearchStores/{}/operations/{}`) until it names its document.
    (
        "GET /{}/{operationName}",
        &["FileSearchStores::import_file"],
    ),
    ("PATCH /{}/credentials/{}", &["Credentials::update"]),
    ("PATCH /{}/triggers/{}", &["Triggers::update"]),
    ("PATCH /{}/webhooks/{}", &["Webhooks::update"]),
    ("POST /{}/agents", &["Agents::create"]),
    ("POST /{}/credentials", &["Credentials::create"]),
    ("POST /{}/environments", &["Environments::create"]),
    ("POST /{}/fileSearchStores", &["FileSearchStores::create"]),
    (
        "POST /{}/fileSearchStores/{}:importFile",
        &["FileSearchStores::import_file"],
    ),
    ("POST /{}/interactions/{}/cancel", &["Interactions::cancel"]),
    ("POST /{}/triggers", &["Triggers::create"]),
    ("POST /{}/triggers/{}/executions", &["Triggers::run"]),
    ("POST /{}/voices", &["Voices::create"]),
    ("POST /{}/webhooks", &["Webhooks::create"]),
    ("POST /{}/webhooks/{}:ping", &["Webhooks::ping"]),
    (
        "POST /{}/webhooks/{}:rotateSigningSecret",
        &["Webhooks::rotate_signing_secret"],
    ),
    (
        "POST /upload/{}/fileSearchStores/{}:uploadToFileSearchStore",
        &["FileSearchStores::upload"],
    ),
    ("POST /upload/{}/files", &["Files::upload"]),
    (
        "PUT /upload/{}/environments/{}/files/{}",
        &["EnvironmentFiles::upload"],
    ),
];

/// Endpoints no handle verb calls, each with its reason.
const ENDPOINTS_WITHOUT_A_VERB: &[(&str, &str)] = &[
    (
        "GET /{}/files/{}:download",
        "files.download: only generated files download, and an uploaded one is a \
         `400 Only GENERATED files can be downloaded` (live 2026-11-05, \
         docs/INTERACTIONS_API_GAP.md \"Resource residue\").",
    ),
    (
        "GET /{}/{clean_id}",
        "file_search_stores.download_media (`GET {media_id}?alt=media`): it needs a \
         `media_id` from a `file_citation`, which the API had not served by \
         2026-11-05 (docs/INTERACTIONS_API_GAP.md \"Resource residue\").",
    ),
    (
        "POST /{}/files:register",
        "files.register_files registers Cloud Storage files with OAuth credentials, \
         and an API key gets `401 API keys are not supported` (live 2026-11-05, \
         docs/INTERACTIONS_API_GAP.md \"Resource residue\").",
    ),
    (
        "POST /{}/interactions",
        "Sent by `client.interaction()...create()` / `create_stream()` \
         (`InteractionBuilder`, not a handle), which every interaction test calls.",
    ),
];

/// A reasoned list in `tests/`: `(file:LIST, [(entry, reason)])`.
type ReasonedList = (String, Vec<(String, String)>);

/// The reason [`reasoned_lists`] gives an entry whose reason is neither a
/// string literal nor a `&str` constant, which the date check cannot read.
const UNREADABLE: &str = "<a reason the scan cannot read>";

/// Every reasoned list in the test sources under `dir`: each `const` or
/// `static` slice of tuples whose last element is a `&str`, at any depth
/// (inside a function or module too), as `(file:LIST, [(entry, reason)])`.
/// A reason is a string literal or a `&str` constant of the same file; an
/// entry is its tuple's first element. Trybuild's `ui/` sources and the
/// fixtures hold no lists and are not read.
fn reasoned_lists(dir: &std::path::Path) -> Vec<ReasonedList> {
    use syn::visit::Visit;

    fn files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                if !path.ends_with("ui") && !path.ends_with("fixtures") {
                    files(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    fn last_is_str(ty: &syn::Type) -> bool {
        let syn::Type::Reference(slice) = ty else {
            return false;
        };
        let syn::Type::Slice(slice) = &*slice.elem else {
            return false;
        };
        let syn::Type::Tuple(tuple) = &*slice.elem else {
            return false;
        };
        tuple.elems.len() >= 2
            && matches!(tuple.elems.last(), Some(syn::Type::Reference(r))
                if matches!(&*r.elem, syn::Type::Path(p) if p.path.is_ident("str")))
    }
    #[derive(Default)]
    struct Consts<'a> {
        strings: std::collections::HashMap<String, String>,
        lists: Vec<(String, &'a syn::Expr)>,
    }
    impl<'a> Visit<'a> for Consts<'a> {
        fn visit_item_const(&mut self, item: &'a syn::ItemConst) {
            self.found(&item.ident, &item.ty, &item.expr);
        }
        fn visit_item_static(&mut self, item: &'a syn::ItemStatic) {
            self.found(&item.ident, &item.ty, &item.expr);
        }
    }
    impl<'a> Consts<'a> {
        fn found(&mut self, name: &syn::Ident, ty: &syn::Type, expr: &'a syn::Expr) {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(text),
                ..
            }) = expr
            {
                self.strings.insert(name.to_string(), text.value());
            } else if last_is_str(ty) {
                self.lists.push((name.to_string(), expr));
            }
        }
        /// A string literal's text, or a `&str` constant's of this file.
        fn text(&self, expr: &syn::Expr) -> Option<String> {
            match expr {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) => Some(text.value()),
                syn::Expr::Path(p) => p
                    .path
                    .get_ident()
                    .and_then(|name| self.strings.get(&name.to_string()).cloned()),
                _ => None,
            }
        }
    }

    let mut paths = Vec::new();
    files(dir, &mut paths);
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path).expect("read test source");
        let file = syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut consts = Consts::default();
        consts.visit_file(&file);
        let relative = path
            .strip_prefix(root())
            .unwrap_or(&path)
            .display()
            .to_string();
        for (name, expr) in &consts.lists {
            let syn::Expr::Reference(array) = expr else {
                continue;
            };
            let syn::Expr::Array(array) = &*array.expr else {
                continue;
            };
            let entries = array
                .elems
                .iter()
                .filter_map(|elem| match elem {
                    syn::Expr::Tuple(tuple) => {
                        let (entry, reason) = (tuple.elems.first()?, tuple.elems.last()?);
                        Some((
                            consts.text(entry).unwrap_or_else(|| "?".to_string()),
                            consts
                                .text(reason)
                                .unwrap_or_else(|| UNREADABLE.to_string()),
                        ))
                    }
                    _ => None,
                })
                .collect();
            out.push((format!("{relative}:{name}"), entries));
        }
    }
    out
}

/// Every reason in a reasoned list anywhere in `tests/` that states a live
/// fact dates it (D-010), as the coverage oracle's do
/// (`tests/api_surface/claims.rs`): the wire oracle's lists, the endpoint
/// map's, the response guard's and the live-verb map's, and any list a
/// test adds.
#[test]
fn live_claims_are_dated() {
    let lists = reasoned_lists(&root().join("tests"));
    let names: BTreeSet<&str> = lists
        .iter()
        .filter_map(|(name, _)| name.rsplit(':').next())
        .collect();
    for known in [
        "ALLOWED",
        "NOT_SENT",
        "FULL_NAMES",
        "ENDPOINTS_WITHOUT_A_VERB",
        "SENT_ONLY",
        "VARIANTS_WITHOUT_EXTRA",
        "NOT_HANDLES",
    ] {
        assert!(
            names.contains(known),
            "the scan missed the reasoned list {known}; it found {names:?}"
        );
    }
    let borrowed: Vec<(&str, Vec<(&str, &str)>)> = lists
        .iter()
        .map(|(name, entries)| {
            let entries = entries
                .iter()
                .map(|(entry, reason)| (entry.as_str(), reason.as_str()))
                .collect();
            (name.as_str(), entries)
        })
        .collect();
    let unreadable: Vec<String> = lists
        .iter()
        .flat_map(|(name, entries)| {
            entries
                .iter()
                .filter(|(_, reason)| reason == UNREADABLE)
                .map(move |(entry, _)| format!("{name}: {entry}"))
        })
        .collect();
    assert!(
        unreadable.is_empty(),
        "Reasons that are neither a string literal nor a `&str` constant of their \
         file, which the date check cannot read:\n  {}",
        unreadable.join("\n  ")
    );
    let undated = claims::undated_live_claims(&borrowed);
    assert!(
        undated.is_empty(),
        "Reasons that state a live fact without its YYYY-MM date (D-010):\n  {}",
        undated.join("\n  ")
    );
}

/// The scan finds a list inside a function, reads a reason held in a
/// constant, and leaves a slice whose last element is not a `&str` alone.
#[test]
fn reasoned_lists_are_found_wherever_they_are_declared() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(
        dir.join("a.rs"),
        r#"
        const WHY: &str = "Rejected live.";
        const TOP: &[(&str, &str)] = &[("a", "Served (live 2026-11-01)."), ("b", WHY)];
        const MAP: &[(&str, &[&str])] = &[("x", &["y"])];
        fn f() {
            const INNER: &[(&str, &[&str], &str)] = &[("c", &[], "Ignored live.")];
        }
        "#,
    )
    .unwrap();
    std::fs::write(dir.join("ui/broken.rs"), "fn {").unwrap();
    let lists = reasoned_lists(dir);
    let names: Vec<&str> = lists
        .iter()
        .map(|(name, _)| name.rsplit(':').next().unwrap())
        .collect();
    assert_eq!(names, ["TOP", "INNER"]);
    assert_eq!(
        lists[0].1,
        [
            ("a".to_string(), "Served (live 2026-11-01).".to_string()),
            ("b".to_string(), "Rejected live.".to_string()),
        ]
    );
    let borrowed: Vec<(&str, Vec<(&str, &str)>)> = lists
        .iter()
        .map(|(name, entries)| {
            let entries = entries
                .iter()
                .map(|(entry, reason)| (entry.as_str(), reason.as_str()))
                .collect();
            (name.as_str(), entries)
        })
        .collect();
    assert_eq!(claims::undated_live_claims(&borrowed).len(), 2);
}

/// `/{api_version}/agents/{agentsId}` -> `/{}/agents/{}`, the form the
/// bindings dump writes.
fn normalize_endpoint(endpoint: &str) -> String {
    let mut out = String::new();
    let mut in_param = false;
    for c in endpoint.chars() {
        match c {
            '{' => {
                in_param = true;
                out.push_str("{}");
            }
            '}' => in_param = false,
            c if !in_param => out.push(c),
            _ => {}
        }
    }
    out
}

/// Every endpoint the bindings and the specs define maps to the handle verbs
/// that call it (and from there, `every_resource_verb_is_called_live_per_handle`
/// requires a live test of each). A baseline bump that adds an endpoint
/// fails here until a verb calls it or it is listed with a reason.
#[test]
fn every_api_endpoint_maps_to_a_handle_verb() {
    let mut endpoints = BTreeSet::new();
    let bindings = snapshot("bindings.json");
    for e in bindings["endpoints"]
        .as_object()
        .expect("bindings endpoints")
        .keys()
    {
        endpoints.insert(e.clone());
    }
    let from_bindings = endpoints.len();
    for file in ["openapi-v1beta.json", "openapi-v1.json"] {
        for e in snapshot(file)["endpoints"]
            .as_array()
            .expect("spec endpoints")
        {
            endpoints.insert(normalize_endpoint(e.as_str().expect("endpoint")));
        }
    }
    assert!(
        from_bindings > 45,
        "parsed only {from_bindings} binding endpoints"
    );
    for legacy in ["GET /{}/files", "GET /{}/fileSearchStores/{}/documents/{}"] {
        assert!(
            endpoints.contains(legacy),
            "the snapshot has no legacy endpoint {legacy}: the dump stopped reading \
             google-genai's hand-written modules"
        );
    }

    let handles = live_verbs::handles();
    let mut problems = Vec::new();
    let mapped: BTreeSet<&str> = ENDPOINTS
        .iter()
        .map(|(e, _)| *e)
        .chain(ENDPOINTS_WITHOUT_A_VERB.iter().map(|(e, _)| *e))
        .collect();
    for endpoint in &endpoints {
        if !mapped.contains(endpoint.as_str()) {
            problems.push(format!(
                "{endpoint}: no ENDPOINTS entry (add the verbs that call it, or an \
                 ENDPOINTS_WITHOUT_A_VERB entry with the reason)"
            ));
        }
    }
    for endpoint in &mapped {
        if !endpoints.contains(*endpoint) {
            problems.push(format!("{endpoint}: not in the snapshot; delete the entry"));
        }
    }
    for (endpoint, verbs) in ENDPOINTS {
        assert!(!verbs.is_empty(), "{endpoint}: no verbs");
        for verb in *verbs {
            let known = verb
                .split_once("::")
                .is_some_and(|(h, v)| handles.verbs.get(h).is_some_and(|verbs| verbs.contains(v)));
            if !known {
                problems.push(format!("{endpoint}: {verb} is not a handle verb"));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "Endpoints vs handle verbs:\n  {}",
        problems.join("\n  ")
    );
}

/// `Handle::verb` pairs no live test needs to call, each with its reason.
const EXEMPT: &[(&str, &str)] = &[];

/// Every public verb on every resource handle is called by a live test, on
/// that handle (`tests/live_verbs/`). A new verb fails here until a live
/// test calls it, or [`EXEMPT`] says why it cannot.
#[test]
fn every_resource_verb_is_called_live_per_handle() {
    let handles = live_verbs::handles();
    let verb_count: usize = handles.verbs.values().map(BTreeSet::len).sum();
    assert!(
        handles.verbs.len() >= 11 && verb_count > 50,
        "parsed too few handles ({}) or verbs ({verb_count}): {:?}",
        handles.verbs.len(),
        handles.verbs.keys()
    );
    let called = live_verbs::live_calls(&handles);

    let mut missing = Vec::new();
    let mut stale = Vec::new();
    for (handle, verbs) in &handles.verbs {
        for verb in verbs {
            let key = format!("{handle}::{verb}");
            let exempt = EXEMPT.iter().any(|(k, _)| *k == key);
            let covered = called.contains(&(handle.clone(), verb.clone()));
            if !covered && !exempt {
                missing.push(key);
            } else if covered && exempt {
                stale.push(format!("{key} (now called live)"));
            }
        }
    }
    for (key, _) in EXEMPT {
        let known = key
            .split_once("::")
            .is_some_and(|(h, v)| handles.verbs.get(h).is_some_and(|verbs| verbs.contains(v)));
        if !known {
            stale.push(format!("{key} (no such verb)"));
        }
    }
    assert!(
        missing.is_empty() && stale.is_empty(),
        "Verbs no live test calls on their handle ({}): add a live test \
         (#[ignore = \"{LIVE_REASON}\"]) that calls each, or an EXEMPT entry \
         with the reason:\n  {}\n\nStale EXEMPT entries ({}):\n  {}",
        missing.len(),
        missing.join("\n  "),
        stale.len(),
        stale.join("\n  ")
    );
}

/// Every handle carries the marker the guard finds handles by: each
/// `Client` accessor's, and each nested one a handle returns.
#[test]
fn resource_handles_carry_the_marker() {
    let handles = live_verbs::handles();
    assert!(
        handles.problems.is_empty(),
        "Resource handles (docs/BUILDER_API.md, \"Resource handles and list \
         builders\"):\n  {}",
        handles.problems.join("\n  ")
    );

    // A handle written without the marker is reported, at the root and
    // nested, while a list builder (it has `send`) is not.
    let file: syn::File = syn::parse_quote! {
        #[must_use = "a resource handle does nothing until you call one of its methods"]
        pub struct Marked<'a> { client: &'a Client }
        pub struct Unmarked<'a> { client: &'a Client }
        pub struct NestedUnmarked<'a> { client: &'a Client }
        pub struct ListThings<'a> { client: &'a Client }
        pub struct InteractionBuilder<'a> { client: &'a Client }
        impl Client {
            pub fn marked(&self) -> Marked<'_> { todo!() }
            pub fn unmarked(&self) -> Unmarked<'_> { todo!() }
            pub fn interaction(&self) -> InteractionBuilder<'_> { todo!() }
        }
        impl<'a> Marked<'a> {
            pub fn nested(self) -> NestedUnmarked<'a> { todo!() }
            pub fn list(self) -> ListThings<'a> { todo!() }
            pub async fn get(self, id: &str) -> Result<Thing, GenaiError> { todo!() }
        }
        impl<'a> ListThings<'a> {
            pub async fn send(self) -> Result<Page, GenaiError> { todo!() }
        }
    };
    let found = live_verbs::handles_in(&[file]);
    let problems = found.problems.join("\n");
    assert!(
        problems.contains("Client::unmarked returns Unmarked"),
        "{problems}"
    );
    assert!(
        problems.contains("Marked::nested returns NestedUnmarked"),
        "{problems}"
    );
    assert_eq!(found.problems.len(), 2, "{problems}");
    assert_eq!(
        found.verbs["Marked"],
        BTreeSet::from(["get", "list", "nested"].map(String::from))
    );
}

/// The receiver typing is what makes the guard per handle: a `get` on one
/// handle must not count for another.
#[test]
fn a_call_counts_only_on_its_own_handle() {
    use syn::visit::Visit;

    let handles = live_verbs::handles();
    let block: syn::Block = syn::parse_quote!({
        let stores = client.file_search_stores();
        stores.create(&request).await;
        client.webhooks().get("id").await;
        assert!(
            client
                .environments()
                .files()
                .list("e", "")
                .send()
                .await
                .is_ok()
        );
        let _ = client.files().get("f");
        self.client.voices().delete("v");
    });
    let mut calls = live_verbs::Calls::new(&handles);
    calls.visit_block(&block);
    let found: BTreeSet<String> = calls
        .found
        .iter()
        .map(|(h, v)| format!("{h}::{v}"))
        .collect();
    let expected: BTreeSet<String> = [
        "FileSearchStores::create",
        "Webhooks::get",
        "Environments::files",
        "EnvironmentFiles::list",
        "Files::get",
        "Voices::delete",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(found, expected);
}

/// An accessor on a receiver of unknown type counts only when it cannot
/// pick the wrong handle. `.files()` is on both `Client` and
/// `Environments`, so on an untracked `envs` (a helper's parameter, say) it
/// counts for neither `Files` nor `EnvironmentFiles`. A receiver named like
/// a client is a `Client`, and an accessor only one type has resolves to
/// its handle.
#[test]
fn an_ambiguous_accessor_on_an_unknown_receiver_counts_for_neither() {
    use syn::visit::Visit;

    let handles = live_verbs::handles();
    let block: syn::Block = syn::parse_quote!({
        envs.files().list("e");
        envs.files().upload("e", "/a.txt", b"x");
        stub.client().files().delete("f");
        harness.webhooks().ping("w");
    });
    let mut calls = live_verbs::Calls::new(&handles);
    calls.visit_block(&block);
    let found: BTreeSet<String> = calls
        .found
        .iter()
        .map(|(h, v)| format!("{h}::{v}"))
        .collect();
    let expected: BTreeSet<String> = ["Files::delete", "Webhooks::ping"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(found, expected);
}

/// A local rebound to something else no longer holds its old handle: by a
/// `let` whose initializer is not a handle, by an assignment, or by a
/// pattern (a `for` loop, a closure parameter). Otherwise a call on the new
/// value would count for the old handle's verb. The initializer is read
/// with the old binding, so `let stores = stores.documents()` counts
/// `FileSearchStores::documents`, and a typed `let` takes the annotation's
/// handle.
#[test]
fn a_rebound_local_drops_its_old_handle() {
    use syn::visit::Visit;

    let handles = live_verbs::handles();
    let block: syn::Block = syn::parse_quote!({
        let hooks = client.webhooks();
        hooks.get("w");
        let hooks = fixtures::hooks();
        hooks.delete("w");

        let mut agents = client.agents();
        agents = make_agents();
        agents.delete("a");

        let voices = client.voices();
        for voices in batches {
            voices.delete("v");
        }
        let creds = client.credentials();
        items.iter().map(|creds| creds.get("c"));

        let stores = client.file_search_stores();
        let stores = stores.documents();
        stores.get("d");

        let envs: Environments<'_> = setup();
        envs.get("e");
    });
    let mut calls = live_verbs::Calls::new(&handles);
    calls.visit_block(&block);
    let found: BTreeSet<String> = calls
        .found
        .iter()
        .map(|(h, v)| format!("{h}::{v}"))
        .collect();
    let expected: BTreeSet<String> = [
        "Webhooks::get",
        "FileSearchStores::documents",
        "FileSearchDocuments::get",
        "Environments::get",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(found, expected);
}
