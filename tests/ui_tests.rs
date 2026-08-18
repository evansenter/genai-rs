#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/fail_*.rs");
    // `pass_*` pins what the macro must *not* require of a consumer — see
    // `tests/ui/pass_no_consumer_imports.rs`. A compile_fail suite alone
    // cannot catch a newly-introduced requirement, because a stricter macro
    // still fails the cases it is supposed to fail.
    t.pass("tests/ui/pass_*.rs");
}
