---
meco: 2
module: cli
entry: line
sampler: weighted/1
inputs:
  playerName: text
imports:
  common: "./common.meco.md"
exports: [line, literal-shell]
---

# line
- [weight = 3, id = line-common] Hello $playerName from @common.place.
- [weight = 1, id = line-alert] Alert $playerName near @common.place!

# literal-shell
- [weight = 1, id = fixed-shell] The patient maintenance pilot waited quietly.
