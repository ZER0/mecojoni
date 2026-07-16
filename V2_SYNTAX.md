# Mecojoni v2 Lexical and Header Grammar

This is the formal companion to the canonical source corpus in `README.md`.
The README remains authoritative for author-facing syntax. This document makes
the implemented source and front-matter boundary precise; production-body EBNF
will be added before that parser phase is considered complete.

## Source representation

- A byte source must be well-formed UTF-8. Decoding never replaces invalid bytes.
- Source spans are half-open and carry both original UTF-8 byte offsets and
  Unicode scalar-value offsets. They do not use UTF-16 code units.
- LF and CRLF are accepted physical line endings. A CR in CRLF remains part of
  the original byte coordinates; both forms become one logical newline when text
  bodies are parsed.
- Identifiers are case-sensitive ASCII. Terminal text after the header may use
  arbitrary Unicode scalar values.
- A module starts at byte zero with the opening header delimiter. A byte-order
  mark, leading blank line, or leading comment is therefore invalid.

## Common tokens

The EBNF uses quoted strings for literal characters, `{x}` for repetition,
`[x]` for optional syntax, and `x | y` for alternatives.

```ebnf
newline          = "\n" | "\r\n" ;
ascii-letter     = "A" … "Z" | "a" … "z" ;
ascii-digit      = "0" … "9" ;
identifier-start = ascii-letter | "_" ;
identifier-rest  = identifier-start | ascii-digit | "-" ;
identifier       = identifier-start, { identifier-rest } ;
space            = " " ;
indent           = space, space ;
```

Tabs are never whitespace in the header. A header line has no trailing spaces.
An inline value follows its colon with exactly one ASCII space. An indented
mapping entry begins with exactly two ASCII spaces and no additional indentation.

## Framing and fields

```ebnf
header           = "---", newline,
                   { blank-line | top-field | section },
                   "---", [ newline ] ;
blank-line       = newline ;

top-field        = version-field
                 | module-field
                 | entry-field
                 | sampler-field
                 | exports-field ;

version-field    = "meco:", space, "2", newline ;
module-field     = "module:", space, identifier, newline ;
entry-field      = "entry:", space, identifier, newline ;
sampler-field    = "sampler:", space,
                   ( "weighted/1" | "diverse/1" ), newline ;
exports-field    = "exports:", space, identifier-list, newline ;

section          = type-section | input-section | import-section ;
type-section     = "types:", newline, { type-entry } ;
input-section    = "inputs:", newline, { input-entry } ;
import-section   = "imports:", newline, { import-entry } ;

type-entry       = indent, identifier, ":", space,
                   nonempty-identifier-list, newline ;
input-entry      = indent, identifier, ":", space, identifier, newline ;
import-entry     = indent, identifier, ":", space, quoted-string, newline ;

identifier-list          = "[", [ identifier-items ], "]" ;
nonempty-identifier-list = "[", identifier-items, "]" ;
identifier-items         = identifier,
                           { ",", { space }, identifier } ;
```

Top-level fields may appear in any order. `meco` and `module` occur exactly once;
all other top-level fields occur at most once. An absent section is an empty
mapping. An absent `exports` is an empty list. Names within one section, list
items, and variants within one finite type are unique.

Unknown fields are errors. The header is deliberately not YAML: tags, anchors,
aliases, merge keys, block scalars, flow mappings, comments, implicit typing, and
coercion are not recognized. In particular, only the unquoted token `2` is a
format version; `2.0`, `"2"`, `02`, and `+2` are invalid.

Every module declares `meco` and `module`. Package validation later enforces that
`entry` and `sampler` are root-only, every entry is exported, imports resolve, and
all package modules use the same language version.

## Header strings

Header strings are currently used for import paths.

```ebnf
quoted-string = '"', { quoted-character | escape }, '"' ;
escape        = "\\\\" | "\\\"" | "\\n" | "\\r" | "\\t" ;
```

A quoted import path must not be empty. An unescaped quote terminates it. Unknown
escapes are errors. Header strings are data for the host loader and are never
interpreted as terminal output or reparsed as Mecojoni source.

## Stable diagnostic families in this phase

The initial header parser exposes these codes:

| Code | Meaning |
| --- | --- |
| `E_HEADER_MISSING` | The source does not start with `---`. |
| `E_HEADER_UNTERMINATED` | The closing delimiter is missing. |
| `E_HEADER_SYNTAX` | A mapping line does not use the exact header grammar. |
| `E_HEADER_INDENT` | Tabs or incorrect nested indentation were used. |
| `E_HEADER_UNKNOWN_FIELD` | A top-level field is not part of format 2. |
| `E_HEADER_DUPLICATE_FIELD` | A field, declaration, variant, or list item repeats. |
| `E_HEADER_REQUIRED_FIELD` | `meco` or `module` is absent. |
| `E_HEADER_VALUE` | A known field has the wrong value shape. |
| `E_UNSUPPORTED_VERSION` | `meco` is not the exact supported integer version. |
| `E_INVALID_IDENTIFIER` | A name is outside the ASCII identifier grammar. |

Parser-independent files under
`crates/mecojoni-core/tests/fixtures/{valid,invalid,packages,expected}` exercise
this contract through `std::fs`, while the production parser remains
`no_std + alloc`.
