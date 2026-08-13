---

# Kairo v0.2 additions: Int, Bool, arithmetic, comparison, if/else

## New tokens
- Keywords: `if`, `else`, `true`, `false`
- Int literals: `[0-9]+` (no floats, no negative literals yet — `-5` is unary-minus-free for now; deferred)
- Operators: `-` `*` `/` `==` `!=` `<` `>` `<=` `>=`

## New expression precedence (lowest to highest)
1. equality: `==` `!=`
2. comparison: `<` `>` `<=` `>=`
3. term: `+` `-`
4. factor: `*` `/`
5. primary: literals, identifiers, calls

## New statements
- `if <expr> { ... }`
- `if <expr> { ... } else { ... }`

## Explicitly deferred
- Parenthesized grouping expressions `(1 + 2) * 3`
- `while` loops
- Unary minus / not
- Float literals
- `else if` chains (must nest explicit `else { if ... }` for now)
---

# Kairo v0.3 additions: while loops, grouping expressions

## New tokens
- Keyword: `while`

## New statements
- `while <expr> { ... }`

## New expressions
- `( <expr> )` — grouping, highest precedence, above primary literals