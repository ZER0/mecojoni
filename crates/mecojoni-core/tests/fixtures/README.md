# Integration fixture conventions

- `valid/` contains standalone sources that must compile.
- `invalid/` contains malformed standalone sources paired with diagnostics in
  `expected/` once the relevant compiler phase exists.
- `packages/` contains directory-based, multi-module packages. The package root
  is always named `root.meco.md`; imports are resolved relative to it.
- `expected/` contains stable diagnostic or generated-output records. Each record
  names its source fixture and the compatibility version that produced it.

Parser fixtures cover exact single diagnostics as well as ordered recovery from
multiple independent errors. Cooked and raw block fixtures assert both normalized
text and parsed interpolation parts. Compiler fixtures cover host-resolved
multi-file packages, visibility and cycle failures, fixed weighted seed corpora,
relative-frequency checks, and a 2,048-rule heap-stack chain.

Tests must read these artifacts through `std::fs`. The production core receives
owned source modules from the host and never performs filesystem I/O.
