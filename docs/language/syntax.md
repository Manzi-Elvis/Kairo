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

---

# Kairo v0.4 additions: mut, real assignment

## New tokens
- Keyword: `mut`
- Operator: `=` (plain assignment, distinct from `:=` and `==`)

## New statement forms
- `mut name := <expr>` — declares a **mutable** variable
- `name := <expr>` — declares an **immutable** variable (unchanged from v0.1)
- `name = <expr>` — reassigns an existing **mutable** variable

## New error cases
- `:=` on a name already declared in the same scope → error (no shadowing yet)
- `=` on a name that was never declared → error
- `=` on a name declared without `mut` → error (immutable)

---

# Kairo v0.5 additions: user-defined functions

## New tokens
- Keyword: `return`
- Punctuation: `,` (comma), `:` (colon, distinct from `:=`), `->` (arrow)

## New function syntax

```
fn name(param: Type, param2: Type) -> ReturnType { ... }
```

- Parameters are comma-separated `name: Type` pairs (zero or more)
- `-> ReturnType` is optional; omitting it means the function returns Unit
- Type names are parsed but not statically checked yet (no type-checker
  pass exists until Phase 3) — mismatches surface as runtime TypeErrors
  the same way untyped values already do

## New statement
- `return <expr>` — returns a value from the current function
- `return` (bare, immediately before `}`) — returns Unit

## New value
- `Unit` — the "no meaningful value" result of a function with no
  return statement, or a bare `return`

## Semantics
- Functions do not close over the caller's variables (no closures yet)
  — each call gets a fresh, empty variable scope containing only its
    parameters
- Recursion is supported (functions can call themselves and each other)
- Calling a function with the wrong number of arguments is a runtime error

---

# Kairo v0.6 additions: structs

## New tokens
- Keyword: `struct`
- Punctuation: `.` (dot, for field access)

## New top-level declaration

```
struct Name {
field: Type,
field2: Type
}
```

Fields are comma-separated `name: Type` pairs (zero or more). This
deviates from the spec's newline-separated example (section 14) —
comma-separated matches the convention already used for function
parameters, and no significant-whitespace handling exists in the
lexer yet.

## New expressions
- `Name { field: <expr>, field2: <expr> }` — struct literal.
  All declared fields must be supplied exactly once; unknown
  or missing fields are errors.
- `<expr>.field` — field access, chainable (`a.b.c`)

## Ambiguity: struct literals in if/while conditions
`if x { ... }` is ambiguous between "condition `x`, then a block"
and "condition is the struct literal `x { ... }`". Kairo resolves
this the same way Rust does: struct literals are **not** parsed
directly inside `if`/`while` conditions. Wrap in parentheses if
one is genuinely needed there: `if (x { ... }) { ... }`.

## Explicitly deferred
- Field mutation (`obj.field = value`) — structs are read-only
  after construction for now; only whole-variable reassignment
  via `=` (if the variable is `mut`) is possible
- Struct methods
- Generic structs