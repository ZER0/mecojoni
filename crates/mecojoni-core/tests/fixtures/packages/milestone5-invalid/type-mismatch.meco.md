---
meco: 2
module: invalid
entry: start
inputs:
  count: number
exports: [start]
---

# start
- @line <- name: $count

# line <- name: text
- $name
