# JavaScript integration artifacts

`hello.mecob` is the canonical `bytecode/1` artifact produced from `examples/hello.meco`. Deno and
browser tests load these checked-in bytes through the handwritten ABI; the CLI integration suite
separately proves regeneration.
