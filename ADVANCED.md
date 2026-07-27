# 🤖 AI Agent & Piping Integration

CovOpt is designed for Unix command chaining and AI Agent workflows.

### Piping into `jq`
When `--json` is passed, all diagnostic logs stream to `stderr`, leaving `stdout` with clean, machine-readable JSON:
```bash
covopt audit --json | jq '.targets[] | select(.passed == false)'
```

### SARIF Report for GitHub Actions
Generate SARIF v2.1.0 output for inline PR annotations in CI:
```bash
covopt report --format sarif
```

---

## 📖 Recommended Workflows

### 🧑 For Humans (Interactive Development)
- **`covopt init --hook`**: Install a fast git pre-commit hook.
- **`covopt fix`**: Auto-fix Clippy warnings and wrap magic numbers.
- **`covopt advise`**: Get instant warnings on hot-path allocations and lock contention.
- **`covopt profile`**: Profile CPU hotspots and visualize SVG flamegraphs.

### 🤖 For AI Coding Agents (Antigravity / Cursor / CI)
- **`covopt ci`**: Unified one-shot pipeline for self-healing and validation.
- **`covopt audit --json`**: Structured JSON APIs for automated parsing.
- **`covopt advise --diff main`**: Analyze PR diffs for complexity regressions.
