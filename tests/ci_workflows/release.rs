//! A release publishes only what CI passed on every platform (#504).

use super::{command_lines, jobs, read, root, steps};

/// release.yml validates on Ubuntu only, so the tagged commit's push run of
/// rust.yml on main, which runs the Cross Platform legs and the rest, must
/// be green on every job before anything publishes: `validate` runs
/// `check_release_push_run.sh` on the tagged commit, with the token able
/// to read Actions, and `publish` needs `validate`.
#[test]
fn a_release_publishes_only_a_commit_green_on_its_main_push_run() {
    let release = read(&root().join(".github/workflows/release.yml"));
    let all = jobs(&release);
    let job = |id: &str| {
        all.iter()
            .find(|(job, _)| job == id)
            .map(|(_, body)| body.clone())
            .unwrap_or_else(|| panic!("release.yml has no {id} job"))
    };
    let validate = job("validate");
    let check = steps(&validate)
        .into_iter()
        .find(|step| command_lines(step).any(|line| line.contains("check_release_push_run.sh")))
        .expect("validate runs .github/scripts/check_release_push_run.sh");
    let call = command_lines(&check)
        .find(|line| line.contains("check_release_push_run.sh"))
        .unwrap_or_default()
        .to_owned();
    assert!(
        call.contains("\"$GITHUB_SHA\"") || call.contains("${{ github.sha }}"),
        "the check reads the tagged commit's run: {call}"
    );
    assert!(
        check.contains("GH_TOKEN: ${{ github.token }}")
            || check.contains("GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}"),
        "the check's gh calls need the job's token:\n{check}"
    );
    assert!(
        !check.contains("        if:") && !check.contains("continue-on-error"),
        "the check runs on every release and its failure stops it:\n{check}"
    );
    assert!(
        validate.contains("      actions: read"),
        "validate's token must read Actions for `gh run`:\n{validate}"
    );
    assert!(
        command_lines(&job("publish")).any(|line| line == "needs: validate"),
        "publish waits for validate"
    );
    assert!(
        root()
            .join(".github/scripts/tests/test_check_release_push_run.sh")
            .exists(),
        "the check has its harness"
    );
}
