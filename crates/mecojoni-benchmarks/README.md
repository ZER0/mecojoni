# Mecojoni benchmark harness

This non-published `std` crate defines the shared `workloads/1` topology sources,
their exact operation contract, and the native release measurement binary.

```sh
cargo +1.85.0 test -p mecojoni-benchmarks --all-targets
cargo +1.85.0 run -p mecojoni-benchmarks --release
cargo +1.85.0 run -p mecojoni-benchmarks --release -- --contract
```

The library and fixtures are safe. The binary alone uses a dependency-free
counting `GlobalAlloc` wrapper around `System` to measure allocation calls and
logical bytes. See the root `BENCHMARKS.md` for interpretation, WASM telemetry,
and retained before/after evidence.
