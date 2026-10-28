# Oxlint demo

A backend-only Convex project linted with
[Oxlint](https://oxc.rs/docs/guide/usage/linter) instead of ESLint, using
`@convex-dev/eslint-plugin` through Oxlint's ESLint-compatible JS plugin
support. `oxlint.config.ts` shows the setup that
[docs.convex.dev/eslint](https://docs.convex.dev/eslint) documents.

`schema.ts`, `messages.ts`, and `crons.ts` each contain a pattern that a
recommended rule reports, suppressed with an `oxlint-disable-next-line` comment.
`digest.ts` only exists as the cron's target. `npm test` runs Oxlint with
`--report-unused-disable-directives`, so a rule that stops firing under Oxlint
fails the test through its now-unused directive. CI runs it via
`turbo run test`.

Type-aware rule behavior (autofixes for `explicit-table-ids`,
`no-collect-in-query`) is unavailable because Oxlint's JS plugins don't expose
TypeScript type information.
