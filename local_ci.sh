#!/bin/bash

# Exit immediately if a command exits with a non-zero status.
set -e

echo "🚀 Starting Local CI Verification for CovOpt-Analyzer..."

echo "------------------------------------------------"
echo "1. Code Quality: Checking formatting (cargo fmt)"
echo "------------------------------------------------"
cargo fmt -- --check

echo "------------------------------------------------"
echo "2. Code Quality: Linting (cargo clippy)"
echo "------------------------------------------------"
cargo clippy --all-targets --all-features -- -D warnings

echo "------------------------------------------------"
echo "3. Compilation: Building Release Binary"
echo "------------------------------------------------"
cargo build --release

echo "------------------------------------------------"
echo "4. Testing: Running Unit Tests"
echo "------------------------------------------------"
cargo test

echo "------------------------------------------------"
echo "5. Testing: Running End-to-End (E2E) Verification"
echo "------------------------------------------------"
# 儲存原本專案的絕對路徑，方便待會呼叫編譯好的執行檔
PROJECT_ROOT=$(pwd)
ANALYZER_BIN="$PROJECT_ROOT/target/release/CovOpt-Analyzer"

# 建立一個暫存資料夾作為 dummy crate 的位置
TMP_DIR=$(mktemp -d)
# 確保腳本結束時 (無論成功失敗) 都會清掉暫存資料夾
trap "rm -rf $TMP_DIR" EXIT

cd "$TMP_DIR"
cargo new --quiet dummy_crate
cd dummy_crate

# 寫入包含簡單 O(N) 迴圈的測試程式碼
cat << 'EOF' > src/lib.rs
#[inline(never)]
pub fn process_data(n: usize) {
    let mut sum = 0;
    for i in 0..n {
        sum += std::hint::black_box(i);
    }
    std::hint::black_box(sum);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_process_complexity() {
        let n: usize = std::env::var("COVOPT_N")
            .unwrap_or_else(|_| "10".to_string())
            .parse()
            .unwrap();
        process_data(n);
    }
}
EOF

echo "=> Running CovOpt-Analyzer on dummy_crate..."
# 執行 CovOpt-Analyzer 進行複雜度測試
"$ANALYZER_BIN" --test test_process_complexity --expected ON --n-values "1000,5000,10000" --target-file src/lib.rs --target-line 5

echo "------------------------------------------------"
echo "✅ All local CI checks passed successfully! 🎉"
echo "------------------------------------------------"
