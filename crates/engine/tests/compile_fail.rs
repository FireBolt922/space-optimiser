//! The public execute API accepts a plan id and a token, never a path
//! (phase 3, review I12).

#[test]
fn execute_plan_cannot_take_a_path() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
