---
meco: 2
module: audit
entry: line
exports: [line, literal-shell, localized]
inputs:
  playerName: text
---

# line
- [weight = 1, id = line-shell] {name as hero}
  @opening $playerName and $hero @suffix@empty.

# opening
- [weight = 1, id = fixed-opening] Fixed opening words

# suffix
- [weight = 1, id = suffix-alpha] alpha
- [weight = 1, id = suffix-beta] beta

# empty
- [weight = 1, id = empty-tail] ""

# name
- [weight = 1, id = bound-name] Ada

# literal-shell
- [weight = 1, id = audit-shell] The patient maintenance pilot waited quietly.

# localized
- [weight = 1, id = localized-arrival] &arrival <- name: $playerName
