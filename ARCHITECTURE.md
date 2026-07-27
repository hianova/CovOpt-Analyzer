# 🛠️ Architecture & Tech Stack

CovOpt-Analyzer is built with a high-performance, modular Rust architecture:

| Domain | Technologies & Libraries |
| :--- | :--- |
| **Core & CLI** | Rust (Edition 2024), `clap` v4 (Derive CLI parser), Modularized Crate (`CovOpt-Analyzer`, `covopt-macro`) |
| **AST & Code Manipulation** | `syn` (AST parsing & visitor traversal), `quote` & `proc-macro2` (AST mutation & macro generation) |
| **Coverage & Dynamic Analysis** | LLVM Source-Based Coverage (`-C instrument-coverage`), `llvm-profdata`, `llvm-cov`, `lcov` parser |
| **Profiling & Assembly** | LLVM-MCA (LLVM Microarchitecture Analysis for IPC & execution ports), `cargo flamegraph` (SVG parser), `samply` |
| **Hardening & Security** | `cargo-mutants` (Mutation Testing), `cargo-fuzz` (Fuzzing), LLVM Sanitizers (`ASan`/`TSan`) |
| **AI Agent & CI Integration** | `serde` / `serde_json` (Structured JSON API), SARIF v2.1.0 (GitHub Actions PR Annotations) |
| **Parallelism & Storage** | `rayon` (Bounded thread pool), `tempfile` (Isolated sandbox execution) |
