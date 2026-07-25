# Challenger 2 Context: Adversarial Anti-DCE & Zero-Entropy Audit

Inspect all benchmark target fixtures in `covopt_cli/tests/` and source code in `covopt_core` / `covopt-macro`:
- Stress test `std::hint::black_box()` usage on loop variables to guarantee LLVM cannot optimize loops away (Anti-DCE).
- Verify no magical constants exist without `covopt_param!`.
- Test `covopt ci --fast` pipeline execution.
Write report to `.agents/teamwork_preview_challenger_m2_2/handoff.md`.
