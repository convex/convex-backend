# Changelog

## 5.0.1

- Fix false positives in `@convex-dev/no-collect-in-query` with type-aware
  linting. In projects that include Convex's types, the rule reported every
  `.collect()` call, including ones on classes and objects unrelated to Convex.
  It now only reports `.collect()` on Convex queries. Thanks to
  [@stickerdaniel](https://github.com/stickerdaniel) for the fix.
- Fix false positives in `@convex-dev/no-filter-in-query` with type-aware
  linting. The rule recognized Convex queries partly by name, so it reported
  `.filter()` on unrelated types, such as a class whose predicate takes a
  parameter of a type named `FilterBuilder`, or a non-Convex `db.query()` result
  stored in a variable. It now only reports `.filter()` on Convex queries.

## 5.0.0

- Add a new rule `@convex-dev/no-process-env` (enabled by default as an error)
  that flags `process.env` in Convex code and, where it can, autofixes it to the
  typed `env` object exported by `_generated/server`. This is a major version
  bump because the new default-enabled rule can surface new errors in existing
  codebases.
- Add a new rule `@convex-dev/no-duplicate-indexes` (enabled by default as an
  error) that detects cases where indexes duplicate the same order of fields and
  can be combined.
- Add a new rule `@convex-dev/require-access-control` (disabled by default) that
  ensures that every public Convex function calls an access control helper (e.g.
  `requireAdmin`, `assertIsLoggedIn`, etc).

## 4.0.0

- Add a new rule `@convex-dev/no-schema-import-cycle` (enabled by default as an
  error) that detects cases where `schema.ts` imports files that themselves
  depend on the schema.

## 3.0.0

- Add a new rule `@convex-dev/no-top-of-hour-crons` (enabled by default as a
  warning) that flags cron jobs scheduled exactly on the hour. The top of the
  hour is the busiest time on the clock for most apps, so moving background work
  off-peak (or omitting `minuteUTC` to let Convex pick a minute) helps your app
  scale. This is a major version bump because the new default-enabled rule can
  surface new warnings in existing codebases.
- The `@convex-dev/no-collect-in-query` and `@convex-dev/explicit-table-ids`
  rules now work when type-aware linting is disabled.
- When type-aware linting is disabled, error messages now recommend enabling it.

## 2.0.0

- Add a new rule `@convex-dev/no-filter-in-query` (enabled by default as a
  warning).

## 1.2.2

- Updated `@typescript-eslint/utils` to v8.58.0 to properly support ESLint v10.

## 1.2.1

- The plugin now supports ESLint v10.x.

## 1.2.0

- Added a new rule `@convex-dev/no-collect-in-query`

## 1.1.1

- Updated the dependency on `@typescript-eslint/utils` to fix a warning when
  installing the ESLint plugins with modern versions of TypeScript.

## 1.1.0

- Added a new rule `@convex-dev/explicit-table-ids`
