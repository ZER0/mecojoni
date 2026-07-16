---
meco: 2
module: arrivals
entry: arrival
inputs:
  itemCount: number
exports: [arrival]
---

# arrival
- {name as hero}
  &arrival <- $hero, count: $itemCount

# name
- [3] Ada
- [1] Marek
