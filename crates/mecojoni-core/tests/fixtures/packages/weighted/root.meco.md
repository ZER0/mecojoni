---
meco: 2
module: weighted
entry: scene
imports:
  common: "./common.meco.md"
exports: [scene, empty, recursive, literals, raw-block]
---

# scene
- [3] @common.person found @item.
- [1] |-
  A quiet scene.

# item
- key
- map
- lantern

# empty
- [1] ""
- [1] something

# recursive
- [5] @item
- [1] @item, @recursive

# literals
- "quoted "r"@raw"

# raw-block
- |raw-
  @literal
