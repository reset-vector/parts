# Contributing

Pull requests are welcome, from people and from agents alike, if every change
to a part's behaviour is backed by provenance.

## Provenance

Provenance is the document a behaviour comes from, cited closely enough that a
reviewer can find the sentence, table or figure without searching:

- a data sheet, programming or service manual, or application note: its title,
  maker, part or document number, and page, section, table or figure;
- a schematic, die photograph or decap: what it shows and where it is held;
- a measurement on real hardware: the part's marking and date code, the
  board, the setup, and the trace or the raw numbers.

Put the citation in the pull request description; the maintainers carry it
into the record each part's behaviour is checked against.

Another emulator is not provenance, and its code is not taken: cite the
document it follows instead. A difference from one with no document behind it
is welcome as an issue, not as a pull request.

A bug fix that does not change a part's documented behaviour, such as a panic, a
build fault or a test, needs no citation beyond the failing case.

## Licence

By opening a pull request you agree that your contribution is licensed under
the MIT licence in `LICENSE`.
