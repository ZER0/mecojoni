---
meco: 2
module: scene
entry: arrival

types:
  Mood: [calm, tense]

inputs:
  playerName: text
  mood: Mood
  urgency: number
  enabled: boolean

imports:
  common: "./common.meco.md"

exports: [arrival, recursion]
---

# arrival
- {mood is tense and enabled}
  {common.name as hero}
  {common.companion <- owner: $hero as companion}
  @alert <- $hero, $companion, level: $urgency
- {mood is calm or not enabled}
  {common.name as hero}
  @calm <- name: $hero

# alert <- hero: text, companion: text, level: number
- [weight = level * 2] $hero and $companion warn $playerName.
- [1] $hero quietly watches $companion.

# calm <- name: text
- @inner <- innerName: "the crew", outerName: $name

# inner <- innerName: text, outerName: text
- $innerName welcomes @{common.name as witness}; $witness greets $outerName.

# recursion
- @recursive-frame <- label: "outer", continue: true

# recursive-frame <- label: text, continue: boolean
- {continue} @recursive-frame <- label: "inner", continue: false
- {not continue} $label
