---
meco: 2
module: localized
entry: greeting
inputs:
  playerName: text
exports: [greeting]
---

# greeting
- &welcome <- name: $playerName
