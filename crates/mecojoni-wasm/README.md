# mecojoni-wasm

This crate exposes the handwritten ABI-1 `wasm32-unknown-unknown` adapter. It owns
the target global allocator, validates every linear-memory range, uses monotonic
opaque handles, returns ordinary language failures as wire results, and requires
explicit disposal.

ABI-1 operations 14 and 15 load and inspect externally supplied `bytecode/0`
bytes. Loaded artifacts return the ordinary grammar handle, so all weighted,
typed, message, diverse, snapshot, disposal, and telemetry APIs remain shared.

The generic build contains no application grammar. The dedicated
`deno task wasm:embedded:build` command selects one already resolved `.mecob`,
copies it through `OUT_DIR`, and exposes it through ABI operation 16. Cargo tracks
both the environment selection and exact artifact path for cache invalidation.

Host-visible allocation count/bytes and live-handle telemetry support leak and
warm-memory tests. The browser-neutral TypeScript owner in `js/mecojoni.ts`
copies result payloads before disposal and is tested unchanged in Deno and Chrome.

```sh
cargo +1.85.0 build -p mecojoni-wasm --target wasm32-unknown-unknown --release
deno task wasm:test
deno task wasm:bench
```

See `V2_INTERFACES.md` for operation numbers, payload ownership, snapshots, and
the additive ABI compatibility policy.
