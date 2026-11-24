# Rust crates

## Development workflow

```sh
# Quick compile check
cargo check -p <package>

# When a change is ready
just lint-rust <package>
cargo test -p <package>
cargo test -p <package> "test_name" # for a specific test or test group
```

Integration and smoke suites run in CI. On Linux the dev setup doesn't install
Postgres or MySQL, so tests that open them (the `postgres` and `mysql` crates,
Big Brain) also run in CI.

## Compatibility with older clients

Projects keep running old versions of the `convex` npm package and its CLI, and
of the other client libraries, for months. Some of them validate responses
strictly, as the CLI does with zod, and fail on a missing field. Any serialized
type in a response to those clients, such as `SerializedComponentInstantiation`,
is a wire format shared with every version still in use:

- Keep obsolete fields required only for compatibility, marked `#[deprecated]`
  and set to an empty value only when older clients accept it; preserve fields
  they still use. A field validated as `z.nullable(...)` must still be sent as
  `null`: a missing key fails validation.
- Before removing or renaming a field, check how older published versions read
  it (for example with `npm pack convex@<version>`), not only the code in this
  repository.

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
- Never hand-write mechanical `match`-based methods on enums — `name()`,
  `label()`, `as_str()`, string parsing, lists of every variant — when a derive
  produces the same mapping. Use `strum` derives for enum↔string conversions
  (`IntoStaticStr`, `Display`, `EnumString`, usually with
  `#[strum(serialize_all = "snake_case")]`) and for iterating variants
  (`EnumIter`), and `serde` derives for serialization. If an impl is a pure
  function of the variant names, derive it.
