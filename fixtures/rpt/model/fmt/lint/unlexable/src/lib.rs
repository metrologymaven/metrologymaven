// Fixture for tools/rpt_float_lint.py: an unterminated string must be a finding (fail
// closed), never a pass.
pub fn broken() -> &'static str {
    "never closed
}
