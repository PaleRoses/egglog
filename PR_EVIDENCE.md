macOS arm64, Rust 1.91.0; upstream `b912caa9694c0c029f9ae3d91db0adc8824e14f5`. Both builds use identical inputs and settings. The chart measures preparation, reconstruction and extractor destruction; graph setup is excluded.

| Program | Upstream → patched | Wall time |
| --- | ---: | ---: |
| eqsolve | 7.13 → 6.90 ms | -3.3% |
| math | 35.93 → 37.23 ms | +3.6% |
| container-rebuild | 4.22 → 4.29 ms | +1.6% |
| extract-vec-bench | 245.14 → 266.06 ms | +8.5% |
| uf-extraction | 3.46 → 3.34 ms | -3.4% |
| taylor51 | 772.04 → 417.07 ms | -46.0% |

All six command outputs match. Results use six interleaved process medians (API) or means (CLI), 20 API samples or 3 warmups/15 CLI runs per process. See [raw timing results](results/benchmarks/cli-analysis.json) for variance. The existing in-process harness separately checks the three smallest controls, excluding process startup: [results](results/benchmarks/native/analysis.json). Nineteen workload controls also compare complete costs and term DAGs: [results](results/benchmarks/holdouts/summary.json).

The first final matrix measured the vector command 8.5% slower and the in-process container control 7.1% slower. A bounded three-arm repeat (upstream, before constant-row pruning, final), using the same input paths and working directory, measured final at 2.2% faster for vector and 9.9% faster for container. Removing only vector's extraction command gave a 1.4% difference; final versus the pre-pruning version was 3.8% faster with extraction and 0.6% slower without it. The initial regressions did not reproduce, but the between-process variance is too large to claim small gains on these controls. Both runs are retained; no source or threshold was tuned after either measurement. [Repeat results](results/controls-recheck/analysis.json).

The dependency index temporarily stores rows and reverse edges. A delayed branch can trigger indexing of other already-settled tables. Fresh mixed-graph allocation checks, with two depth-eight chains plus 0/1,000/10,000 settled rows, show:

| Case | Peak additional requested bytes | Retained prepared bytes |
| --- | ---: | ---: |
| mixed-adverse-0 | 9,473 → 8,069 B | 6,981 → 4,676 B |
| mixed-adverse-1000 | 9,424 → 8,069 B | 6,996 → 4,684 B |
| mixed-adverse-10000 | 9,424 → 8,069 B | 6,996 → 4,684 B |
| mixed-favorable-0 | 9,792 → 6,721 B | 6,981 → 4,676 B |
| mixed-favorable-1000 | 9,756 → 6,673 B | 6,996 → 4,684 B |
| mixed-favorable-10000 | 9,756 → 6,673 B | 6,996 → 4,684 B |

The earlier rehearsal exposed unnecessary copies of constant rows (532 KB at 10,000 rows). The correction uses the existing dependency walk to exclude rows with no equality leaves. Rows with equality dependencies still require temporary row and reverse-edge storage. These are requested Rust allocation bytes, not RSS. Instrumented durations are excluded from timing claims. [Allocation records](results/benchmarks/mixed/summary.json).

Separate uninstrumented repetitions of these six mixed cases were 1.05–2.06× faster in total. [Timings](results/benchmarks/mixed-timing/summary.json). A separate fixture with an already-known first child and delayed second child, directly and through a map key/value pair, also matched upstream's costs and terms. [Check](results/order-sensitive/status.json).

**Reproduce:** use separate upstream/patched checkouts with the same `benches/rust_api_benchmarking.rs` and separate Cargo target directories. In each, run:

```sh
export CARGO_BUILD_JOBS=1 RAYON_NUM_THREADS=1 RUST_LOG=error
export CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=false
cargo +1.91.0 bench --locked --bench rust_api_benchmarking -- \
  rust_tree_extraction_chain --sample-count 20 --sample-size 1 --threads 1 --timer os
cargo +1.91.0 build --release --locked --bin egglog
hyperfine --warmup 3 --runs 15 'target/release/egglog tests/taylor51.egg'
```

Save the normal CLI after benchmark builds. Run timings without simultaneous builds/tests. Exact local commands and binary hashes: [rehearsal records](results/benchmarks/provenance.json).

**Checks:** `make test`, `make nits`, `cargo check --no-default-features --lib`, `make -C wasm-example test`. 1,315 tests, 37 distinct doctests passed; 8 upstream doctests ignored. [Logs](validation-runs.json). Linux, remote CI/coverage and the full performance matrix were not run.
