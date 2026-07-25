# Challenger 2 Context: Adversarial Stress Test & Edge Case Verification

Perform stress testing on `covopt audit --json`, `covopt ci --fast`, `covopt fix`, `covopt advise`, and `covopt profile`.
Test edge cases (virtual workspace root, missing directories, missing external tools like cargo-mutants/cargo-fuzz).
Verify stdout JSON output of `covopt audit --json` with `jq`.
Write report to `.agents/teamwork_preview_challenger_m1_2/handoff.md`.
