# Italian exercises gender selection and its cardinal one/other categories.
arrival = { $gender ->
    [female] La viaggiatrice { $name } è arrivata
    [male] Il viaggiatore { $name } è arrivato
   *[other] La persona { $name } è arrivata
} con { $count ->
    [one] un oggetto
   *[other] { $count } oggetti
}.
