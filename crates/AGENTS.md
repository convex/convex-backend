# Rust crates

## Development workflow

```sh
# When a change is ready
just lint-rust <package>
cargo test -p <package>
cargo test -p <package> "test_name" # for a specific test or test group
```

```sh
# Before committing or presenting the output
just format-rust
```

## Rust style

- Before adding a crate dependency or a new abstraction, check whether existing
  workspace infrastructure already provides the capability and prefer the
  simplest extension that works.
- Match domain enums exhaustively instead of using a `_ =>` catch-all so adding
  a variant causes a compile error.
- Use self-documenting domain types: prefer named structs over positional
  tuples, enums over boolean flags, and `Duration` or a newtype over bare
  numbers with implicit units.
- Prefer the `anyhow::Context` trait (`.context(...)`/`.with_context(...)`) over
  `.ok_or_else(|| anyhow::anyhow!(...))` or `.map_err(|_| anyhow::anyhow!(...))`
  for attaching a message to a `Result` or `Option`; it says the same thing more
  concisely.
- Express pure type conversions as `From`/`TryFrom` impls rather than inherent
  methods like `into_foo()` or `to_foo()`. Use an inherent method when the
  conversion needs arguments or is non-canonical. Avoid using trait impls for
  complex types like `&'a Foo` or `(Foo, Bar)` unless necessary for generics.
