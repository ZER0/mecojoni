---
meco: 2
module: npc
entry: line
sampler: weighted/1
inputs:
  playerName: text
imports:
  common: "./common.meco.md"
exports: [line]
---

# line
- $playerName, @common.observation.
