# migrations_model

This crate holds backend data migrations that evolve the shape of system tables
owned by `crates/model`.

Each migration lives in its own `migr_<n>/` directory and is pinned to the data
shapes that existed at the time it was written. To preserve that guarantee, the
types it needs are **copy-pasted** out of `crates/model/` into the migration
directory and frozen there — migrations must not import from `crates/model`
directly.

## Adding a new migration

See the `adding-backend-model-migration` Claude skill at
[`.claude/skills/adding-backend-model-migration/SKILL.md`](../../.claude/skills/adding-backend-model-migration/SKILL.md)
for the full step-by-step (creating the `migr_<n>/` directory, copying the
relevant system model, writing the migration in `migr_<n>/mod.rs`, and wiring it
into `lib.rs`).
