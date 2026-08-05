#!/bin/bash
echo "🏁 Starting CovOpt 3.0 Contest: The 4 Trials"
echo "================================================="

echo ""
echo "=== Trial 1.1: The Rediscovery Test (LRU Cache) ==="
cargo run --manifest-path ../CovOpt-Analyzer/Cargo.toml --bin covopt -- evolve --target LruCache
echo ""
echo "=== Trial 1.2: The Rediscovery Test (Fast Inverse Sqrt) ==="
cargo run --manifest-path ../CovOpt-Analyzer/Cargo.toml --bin covopt -- evolve --target fast_inv_sqrt

echo ""
echo "=== Trial 2: Chaos Survival Test (Toxic Gene) ==="
cargo run --manifest-path ../CovOpt-Analyzer/Cargo.toml --bin covopt -- evolve --target ToxicStructure

echo ""
echo "=== Trial 3: Superoptimization Benchmark (Specialized Sort) ==="
cargo run --manifest-path ../CovOpt-Analyzer/Cargo.toml --bin covopt -- evolve --target super_sort

echo ""
echo "================================================="
echo "🏁 Trials 1-3 Completed."
echo ""
echo "=== Trial 4: Dogfooding (Self-Hosting) Benchmark ==="
echo "Benchmarking Trial 1.1 (LRU Cache) with CovOpt 3.0 (Old Matrix):"
time cargo run --manifest-path ../CovOpt-Analyzer/Cargo.toml --bin covopt -- evolve --target LruCache > /dev/null

echo ""
echo "Benchmarking Trial 1.1 (LRU Cache) with CovOpt 3.1 (Evolved Matrix):"
time cargo run --manifest-path ../CovOpt-Analyzer-3.1/Cargo.toml --bin covopt -- evolve --target LruCache > /dev/null
echo ""
echo "🎉 Dogfooding Complete!"
