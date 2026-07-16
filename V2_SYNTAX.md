# Mecojoni v2 Lexical and Source Grammar

This is the formal companion to the canonical source corpus in `README.md`.
The README remains authoritative for author-facing syntax. This document makes
the implemented source, front-matter, and production-body grammar precise.

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
qualified-name   = identifier, { ".", identifier } ;
space            = " " ;
indent           = space, space ;

integer-part     = "0" | ( "1" … "9", { ascii-digit } ) ;
fraction-part    = ".", ascii-digit, { ascii-digit } ;
exponent-part    = ( "e" | "E" ), [ "+" | "-" ],
                   ascii-digit, { ascii-digit } ;
decimal          = [ "-" ], integer-part, [ fraction-part ],
                   [ exponent-part ] ;
```

A `rational/1` decimal contains at most 18 mantissa digits and its parsed exponent
is in `-18..=18`. Leading integer zeroes, a leading decimal point, a trailing
decimal point, `NaN`, infinities, hexadecimal forms, underscores, and implicit
coercions are invalid. Weight metadata admits only a positive static decimal;
dynamic arithmetic may temporarily produce signed exact values but the evaluated
weight must be non-negative.

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

## Rules and productions

The body after the closing header delimiter is a sequence of comments and rules.
A rule owns every following production until the next heading.

```ebnf
module-body      = { blank-line | comment-line | rule } ;
rule             = heading, newline, rule-trivia,
                   production, { rule-trivia, production } ;
rule-trivia      = { blank-line | comment-line } ;
heading          = "#", space, identifier, [ parameter-list ] ;
parameter-list   = space, "<-", space, parameter,
                   { ",", space, parameter } ;
parameter        = identifier, ":", space, identifier ;

production       = "-", space, [ metadata, space ], production-content ;
metadata         = "[", static-weight, "]"
                 | "[", long-metadata, "]" ;
static-weight    = positive-decimal ;
long-metadata    = "weight", space, "=", space, weight-expression,
                   [ ",", space, "id", space, "=", space, identifier ] ;
```

The long metadata fields may appear in either order, but each occurs once and
`weight` is required. A static shorthand must be greater than zero. A long weight
expression may evaluate to zero at generation time, making the production
ineligible. Metadata-looking text at the beginning of a production is always
parsed as metadata and malformed forms are errors; literal leading brackets must
be quoted or raw.

A production may continue on logical lines indented by at least two spaces. The
first two spaces establish production ownership and are removed; further spaces
belong to that logical line. Blank separator lines are not production content.

## Non-emitting clauses

Zero or more braced clauses precede the visible body. Guards precede bindings.

```ebnf
production-content = { clause, clause-separator }, visible-body ;
clause-separator    = space | newline, indent ;
clause              = guard | binding ;
guard               = "{", guard-expression, "}" ;
binding             = "{", qualified-name, space, "as", space,
                      identifier, "}" ;

guard-expression    = guard-or ;
guard-or            = guard-and, { space, "or", space, guard-and } ;
guard-and           = guard-not, { space, "and", space, guard-not } ;
guard-not           = [ "not", space ], guard-primary ;
guard-primary       = "(", guard-expression, ")" | guard-comparison ;
guard-comparison    = guard-value,
                      [ space, guard-operator, space, guard-value ] ;
guard-operator      = "is" | ( "is", space, "not" )
                    | "<" | "<=" | ">" | ">=" ;
guard-value         = identifier | decimal | "true" | "false"
                    | quoted-string ;
```

`not` binds tighter than `and`, which binds tighter than `or`. Bare names are
resolved later against immutable inputs, parameters, and contextually typed enum
members. A guard cannot reference a binding from the same production because all
guards are evaluated before any binding expands.

## Weight expressions

```ebnf
weight-expression = weight-additive ;
weight-additive   = weight-product,
                    { space, ( "+" | "-" ), space, weight-product } ;
weight-product    = weight-primary,
                    { space, "*", space, weight-primary } ;
weight-primary    = decimal | identifier
                  | "(", weight-expression, ")" ;
```

Multiplication binds tighter than addition and subtraction; operators of equal
precedence associate left-to-right. Names refer only to immutable numeric inputs
or rule parameters. `$name`, calls, bindings, captures, messages, arrays, records,
clocks, and callbacks are syntactically or semantically forbidden here.

## Visible bodies and references

```ebnf
visible-body       = empty-body | block-body | inline-body | complete-message ;
empty-body         = '""' ;
inline-body        = body-part, { body-part } ;
body-part          = terminal-text | escaped-character | quoted-string
                   | raw-string | rule-reference | emitting-capture
                   | value-reference | rule-call ;

rule-reference     = "@", qualified-name
                   | "@{", qualified-name, "}" ;
emitting-capture   = "@{", qualified-name, space, "as", space,
                     identifier, "}" ;
value-reference    = "$", identifier | "${", identifier, "}" ;
rule-call          = "@", qualified-name, space, "<-", call-arguments ;
complete-message   = "&", qualified-name,
                     [ space, "<-", call-arguments ] ;

call-arguments     = [ space ], argument,
                     { ( ",", space | newline, indent ), argument } ;
argument           = identifier, ":", space, value | "$", identifier ;
value              = value-reference | decimal | "true" | "false"
                   | quoted-string ;
raw-string         = "r", '"', { raw-character }, '"' ;
```

`$name` in an argument list is punning for `name: $name`. Argument names are
unique. `<-` passes data and emits none of its spelling. A complete `&message`
must own the entire visible body; it cannot be captured, prefixed, suffixed, or
combined with a second visible message. These complete-message effects are
validated transitively by the compiler.

Simple `@name` consumes the longest qualified ASCII name. Use `@{name}` before a
literal identifier-like suffix, as in `@{creature}s`. An emitting capture expands
once, emits once, and makes its value available under the capture name. A braced
binding expands once without emitting.

## Output strings, escapes, and blocks

The output escape table is exact:

| Source | Value |
| --- | --- |
| `\\` | backslash |
| `\"` | double quote |
| `\n` | line feed |
| `\r` | carriage return |
| `\t` | tab |
| `\@` | literal `@` |
| `\$` | literal `$` |
| `\&` | literal `&` |
| `\//` | literal `//` |

Unknown escapes are errors. Quoted strings apply the table and omit their
delimiters. `r"..."` and raw blocks interpret no sigils or escapes. The complete
body `""` is empty output; an empty quoted segment inside another body is rejected.
Leading and trailing output spaces must be inside a quoted or raw literal.

```ebnf
block-body       = block-marker, newline, block-line, { newline, block-line } ;
block-marker     = "|" | "|-" | "|+" | "|raw" | "|raw-" | "|raw+" ;
block-line       = indent, { source-character } ;
```

The production's two-space indentation is removed from each block line and any
additional indentation is preserved. Physical newlines become `\n`. The default
clip mode (`|` or `|raw`) emits one final newline, strip (`-`) emits none, and keep
(`+`) preserves authored trailing indented blank lines plus the final newline.

## Comments

`<!-- ... -->` comments are non-nesting. They may occupy whole unindented lines
between rules/productions or appear between production syntax items; comment bytes
emit nothing. Inside quoted strings, raw strings, and raw blocks the same spelling
is literal text. An unterminated or nested comment is an error.

## AST mapping

| Source form | Parsed node |
| --- | --- |
| `# name <- value: type` | `RuleSyntax` plus `ParameterSyntax` |
| `[3]` | `WeightSyntax::Static` |
| `[weight = urgency * 2]` | `WeightSyntax::Dynamic` expression tree |
| `{mood is tense}` | `ClauseSyntax::Guard` |
| `{common.name as hero}` | `ClauseSyntax::Binding` |
| `@name` / `@{name}` | `BodyPartSyntax::RuleReference` |
| `@{name as hero}` | `BodyPartSyntax::EmittingCapture` |
| `$hero` | `BodyPartSyntax::ValueReference` |
| `@rule <- ...` | `BodyPartSyntax::RuleCall` |
| `&message <- ...` | `BodyPartSyntax::MessageCall` |
| quoted/raw/terminal text | `BodyPartSyntax::Literal` |
| `|...` / `|raw...` | `BodySyntax::Block` |
| `""` | `BodySyntax::Empty` |

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
| `E_COMMENT_SYNTAX` | A comment is nested, unterminated, or misplaced. |
| `E_RULE_SYNTAX` / `E_DUPLICATE_RULE` | A heading/rule is malformed or repeats. |
| `E_PRODUCTION_SYNTAX` | A production or continuation is malformed. |
| `E_WEIGHT_SYNTAX` | Weight metadata or its expression is invalid. |
| `E_CLAUSE_ORDER` | A guard appears after a binding. |
| `E_GUARD_SYNTAX` / `E_BINDING_SYNTAX` | A non-emitting clause is malformed. |
| `E_BODY_SYNTAX` / `E_BLOCK_SYNTAX` | A visible body or block is malformed. |
| `E_STRING_SYNTAX` / `E_ESCAPE_SYNTAX` | A string or escape is invalid. |
| `E_CALL_SYNTAX` / `E_ARGUMENT_SYNTAX` | A call or argument list is invalid. |

Parser-independent files under
`crates/mecojoni-core/tests/fixtures/{valid,invalid,packages,expected}` exercise
this contract through `std::fs`, while the production parser remains
`no_std + alloc`.
