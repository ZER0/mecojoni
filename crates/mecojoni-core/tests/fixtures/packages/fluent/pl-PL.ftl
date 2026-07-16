# Polish exercises one/few/many as well as grammatical-gender selection.
arrival = { $gender ->
    [female] Pani { $name } przybyła
    [male] Pan { $name } przybył
   *[other] { $name } przybyło
} z { $count ->
    [one] jednym przedmiotem
    [few] { $count } przedmiotami
   *[many] { $count } przedmiotami
}.
