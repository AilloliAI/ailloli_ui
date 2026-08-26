# Benchmarking Ailloli UI

Ailloli UI performance gates use structured, reproducible measurements rather
than unqualified timing claims. `ailloli_ui_bench` records versioned JSONL runs
through a bounded writer queue, while the `ailloli-ui-bench` CLI validates and
compares complete native sessions.

The library remains a normal dependency of the renderer and native host so
instrumentation call sites share one stable event contract. Collection is
disabled by default and performs no filesystem write or writer-thread startup
until explicitly enabled. The heavier command-line surface remains opt-in
behind the `cli` feature.

## Recording application measurements

Application runs opt in explicitly and must use a new destination:

```sh
AILLOLI_UI_BENCH=1 \
AILLOLI_UI_BENCH_PATH=artifacts/bench/manual/sandbox.jsonl \
cargo run -p sandbox_app
```

Keep the `BenchInit` guard alive for the complete measured run and call
`finish()` before exit. This makes write, flush, synchronization, and
publication failures observable instead of silently losing measurements.

## Native regression matrix

For reproducible native comparisons, build the measured child once in release
mode, then run it through the feature-gated CLI. Each scenario receives its own
directory, process, `RunEnd`, SHA-256 index, backend, dimensions, and observed
scale factor:

```sh
CARGO_INCREMENTAL=0 cargo build --release --locked \
  -p ailloli_ui_winit --features test_support \
  --example winit_regression_bench

CARGO_INCREMENTAL=0 cargo run --release --locked \
  -p ailloli_ui_bench --features cli --bin ailloli-ui-bench -- \
  run-matrix \
  --output-root artifacts/bench/winit_host_architecture \
  --phase candidate --winit-version 0.30.13 --backend wayland \
  --profile release --harness winit_regression_bench \
  --target x86_64-unknown-linux-gnu --machine local-wayland-01 \
  --scenario wake_single --mode steady \
  --warmups 3 --samples 30 --duration-ms 1200 \
  -- target/release/examples/winit_regression_bench

CARGO_INCREMENTAL=0 cargo run --release --locked \
  -p ailloli_ui_bench --features cli --bin ailloli-ui-bench -- \
  summarize --input \
  artifacts/bench/winit_host_architecture/candidate/winit-0.30.13/wayland/wake_single
```

Run Wayland and X11 separately. Compare only artifacts produced with the same:

- schema and harness version;
- machine and compilation target;
- CPU, GPU, driver, and rendering backend;
- build profile and feature set;
- window geometry, device-pixel ratio (DPR), and observed scale factor;
- scenario, mode, warmup count, sample count, and duration.

Record these values with the result. A performance change without comparable
machine, GPU, backend, geometry, and DPR metadata is not a regression result.

## Correctness and acceptance

The CLI rejects incomplete sessions and correctness counters above zero. A
comparison is valid only when every measured process emits its terminal
`RunEnd`, the SHA-256 index matches the session contents, and the scenario
metadata is complete.

Performance evidence complements functional tests; it never replaces runtime,
renderer, or integration correctness gates. Visual capture tests are also
separate: they validate rendered output and interaction state, not timing.

## Environment compatibility

New integrations use the `AILLOLI_UI_BENCH_*` environment variables. The old
`OCTAVUI_BENCH_*`, `UI_BENCH*`, and `BENCH_*` names remain lower-priority
compatibility fallbacks only. The deprecated append-only `init_from_env` path
is not a regression gate.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the host and renderer boundaries and
[CONTRIBUTING.md](CONTRIBUTING.md) for the complete development gates.
