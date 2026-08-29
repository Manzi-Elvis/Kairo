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

---

# Kairo v0.7: static type checking

`kairo check` and `kairo run` now perform static type checking
after parsing, before execution. Mismatched operand types, wrong
argument types/counts, struct field mismatches, non-Bool
conditions, and undefined types/functions/structs are now caught
as `type error: ...` without needing to run the program.

---

# Kairo v0.8: enums (construction only, no match yet)

## New tokens
- Keyword: `enum`
- Punctuation: `::` (double colon, for `EnumName::Variant`)

## New declaration

```
enum Status {
  Pending,
  Failed(reason: String)
}
```

Variants are comma-separated. A variant with no parentheses carries
no data; a variant with `(name: Type, ...)` carries named fields,
same shape as struct fields.

## New expression
- `EnumName::Variant` — unit variant construction
- `EnumName::Variant(field: <expr>, ...)` — data variant construction,
  same field-matching rules as struct literals (all fields required,
  by name, no extras)

## Deferred to v0.9
- `match` expressions/statements — this slice only supports
  constructing and passing enum values around, not inspecting them
- `Option<T>` / `Result<T, E>` as built-ins — needs generics, which
  don't exist yet; deferred further than v0.9

  ---

# Kairo v0.9: match statements and exhaustiveness

## New tokens
- Keyword: `match`
- Punctuation: `=>` (fat arrow)
- `_` is not a new token — it's the existing identifier `_` treated
  as a wildcard pattern only inside match arms.

## New statement
```
match <scrutinee> {
  EnumName::Variant => { ... }
  EnumName::Variant(binding1, binding2) => { ... }
  5 => { ... }
_   => { ... }
}
```
- No commas between arms — each arm's `{ ... }` delimits it.
- Enum variant patterns bind fields **positionally** by the order
  declared in the `enum`, regardless of the binding name chosen.
- Literal patterns (`Int`, `Bool`, `String`) match by value equality.
- `_` matches anything and binds nothing.

## Exhaustiveness (checked statically by `kairo check`)
- Enum scrutinee: every variant must have an arm, or a `_` arm.
- Bool scrutinee: both `true` and `false` must have arms, or `_`.
- Int/String/Struct/Unit scrutinee: a `_` arm is required (no way
  to enumerate all values).

## Known limitation
Pattern bindings are scoped to their arm by the type checker (not
visible after the match, not shared between arms). The interpreter
still uses one flat per-function variable scope like the rest of
the language, so bindings technically remain accessible after the
match at runtime too. Harmless today since the type checker already
rejects any program that could observe the difference — worth
tightening once real block scoping exists.

## Deferred
- Match as an expression (returning a value)
- Nested/compound patterns, guards (`if` conditions on arms)
- `Option<T>` / `Result<T, E>` built-ins (still blocked on generics)

---

# Kairo v0.10: arrays

## New tokens
- Punctuation: `[`, `]`

## New expressions
- `[<expr>, <expr>, ...]` — array literal, type inferred from elements
  (all elements must share one type; empty literals are not yet
  supported — no way to infer their type without generics syntax)
- `<expr>[<expr>]` — indexing (read), Int index, runtime bounds check

## New statement
- `name[<expr>] = <expr>` — index assignment; `name` must be a `mut`
  variable bound to an array

## New built-in functions
- `len(arr) -> Int`
- `push(arr, item) -> Array<T>` — returns a **new** array with `item`
  appended; does not mutate `arr` (consistent with Kairo's current
  value-semantics-only model — no references exist yet)

## Deferred
- Array types in function parameters/return types (`Array<Int>`) —
  blocked on generic type syntax, which doesn't exist yet
- `for`/`for-in` loops — iterate with `while` + `len` + indexing for now
- Negative indices, slicing, `pop`/`remove`

---

# Kairo v0.11: `?` error propagation (convention-based, not generic)

## New token
- Punctuation: `?`

## New expression
- `<expr>?` — postfix. `<expr>` must have an enum type with exactly
  an `Ok(value: T)` variant and an `Err(error: E)` variant (checked
  by name, not by a compiler-recognized generic type). The current
  function's return type must be that exact same enum. If `<expr>`
  evaluates to `Err(e)`, the function returns that `Err(e)`
  immediately. If `Ok(v)`, the expression evaluates to `v`.

## Known limitation
This is not the spec's generic `Result<T, E>` (section 17) — Kairo
has no generic type syntax yet (the same reason `Array<T>` function
parameters were deferred). Each error-returning function family
needs its own concretely-typed Ok/Err enum declared by the user,
e.g.:

```
enum IntResult {
Ok(value: Int),
Err(error: String)
}
```
`?` works on any enum shaped this way, regardless of its name. Real
generics would let one `Result<T, E>` cover every case — worth
revisiting once generics exist.

---

# Kairo v0.12: modules, imports, export

## New syntax
- `module Name` — optional file header; if present, must match the
  file's name (without `.kairo`), enforced by the CLI
- `import Name` — loads the sibling file `Name.kairo` in the same
  directory and merges its exported declarations in
- `export` — prefix on `struct`/`enum`/`fn` to make it importable

## Visibility rule
A file's own code may use its own declarations freely, exported or
not. To use a declaration from another file, that file must be
directly imported (not just transitively reachable) and the
declaration must be `export`ed. Violating either rule is a
`SymbolNotAccessible` error at `kairo check`/`run` time.

---

# Kairo v0.13: HIR (compiler-internal, no new syntax)

A new `kairo-hir` crate lowers the AST into a desugared High-level
IR (spec section 57), currently exercised as a standalone, tested
compiler pass — not yet wired into `kairo run`/`check` in place of
the AST-walking interpreter.

## What's lowered
- `match` statements → nested `if`/`else` chains using two new HIR
  node kinds: `IsVariant` (tag test) and `VariantField` (positional
  field extraction). The `Pattern` concept from the AST does not
  exist at this level. The match scrutinee is evaluated once into a
  generated temporary (`__match_scrutinee_N`), matching how a real
  compiler avoids re-evaluating it per arm.

## What's NOT lowered (documented limitation)
- `?` (`Expr::Try`) passes through unchanged. Desugaring it requires
  knowing which enum the inner expression evaluates to, which is
  type information — this lowering pass is purely syntactic and has
  no type checker integration. Revisit once HIR carries inferred
  types.
- MIR (ownership, borrows, moves, drops — section 58) is out of
  scope. It only earns its keep in service of a native-codegen
  backend, which does not exist yet (`kairo build` is still a stub).