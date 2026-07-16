# Gender and cardinal-plural selection are deliberately owned by Fluent.
arrival = { $gender ->
    [female] Ms. { $name }
    [male] Mr. { $name }
   *[other] { $name }
} arrived with { $count ->
    [one] one item
   *[other] { $count } items
}.
