# Mecojoni editor grammar

This dependency-free VS Code extension contributes the `source.mecojoni`
TextMate grammar for `.meco` and `.meco.md` files plus comment, bracket, folding,
and auto-closing defaults. Open this directory as an extension development host
or copy it into a local extension package.

The grammar highlights the strict front matter, rules, productions, weights,
comments, raw/cooked strings and blocks, guards/bindings, rule references,
values, messages, and escapes. Semantic diagnostics deliberately come from
`meco check`; an LSP transport is deferred until an editor integration needs
incremental document synchronization rather than inventing a second parser.
