# Kairo v0.1 Lexical Grammar (Milestone Scope)

Scope: only what's needed for §95's hello-world program.

## Tokens

- Keywords: `fn`
- Identifiers: `[a-zA-Z_][a-zA-Z0-9_]*`
- String literals: `"..."` (no escapes yet — deferred)
- Punctuation: `(` `)` `{` `}` `:=` `+`
- Whitespace: space, tab, newline — skipped, newlines tracked for spans
- Comments: `//` to end of line — skipped
- EOF: explicit end-of-file token

## Explicitly deferred (not in v0.1)
- Numeric literals
- String escapes
- All other operators (`-`, `*`, `/`, `==`, etc.)
- All other keywords (`mut`, `if`, `match`, `async`, ...)

## Error cases v0.1 must handle
- Unterminated string literal
- Unrecognized character