# Changelog

## 1.49.0 (unreleased)

- New `npx convex plugins install` command, which installs the Convex plugin for
  Claude Code and Codex on macOS and Linux. It works with the terminal CLIs, the
  desktop apps and the VS Code extensions. Running it again upgrades the plugin
  to the latest version.

## 1.48.0

- Convex deployments can now sign their own JWTs with an internally managed
  key-pair. The `ctx.auth.signJwt` API is available in mutations. The JWTs
  can be verified with the public key in the JWKS hosted at
  `${CONVEX_CLOUD_URL}/.well-known/jwks.json`.
- `npx convex dev --verbose` and `npx convex deploy --verbose` no longer print
  environment variable values. The logged list of environment variables shows
  every value as `<redacted>`, and values of 8 or more characters are also
  redacted wherever else they appear in the logged push response and diff, such
  as auth providers that `auth.config.ts` builds from `process.env`.
- When an AI agent runs `npx convex env list` or `npx convex env default list`,
  they now list only names by default, so secrets stay out of the agent's
  context. Agents are detected from the environment variables they set. Pass
  `--include-values` to print the values.

- Fixed `npx convex ai-files install` and `npx convex ai-files update` reporting
  success and exiting 0 when the guidelines download or the agent skills
  install failed. They now exit with code 1.

- `ctx.storage.store()` and `ctx.storage.get()` are now available in mutations,
  not just actions. Files stored and read in a mutation count toward its
  transaction limits.

## 1.47.0

- Fixed the next step printed by `npx convex project create`. It suggested
  `npx convex deployment create <team>:<project>:dev --type dev`, which always
  failed because `dev` is reserved for your default dev deployment. It now
  suggests a `dev/my-feature` reference and passes `--select`.
- `npx convex deploy` now asks for confirmation before creating or changing an
  index on a large table, since the deploy blocks until the index is backfilled.
  Stage the index (`staged: true`) to backfill it in the background instead, or
  pass the new `--skip-large-indexes-check` flag to proceed without
  confirmation. That flag also covers deleting large indexes, replacing the
  hidden `--allow-deleting-large-indexes` flag, which keeps working.
- The `envList` MCP tool now returns only environment variable names by default,
  so secrets stay out of the agent's context. Pass `includeValues: true` to get
  the values too, or use `envGet` to read a single variable.
- `npx convex env list` and `npx convex env default list` have a new
  `--include-values` flag for printing values explicitly. Values are still
  printed by default. The docs and help text now use `--include-values` wherever
  the values are needed, like
  `npx convex env list --include-values > .env.convex`, and `--names-only` for
  just listing variables.
- Added `.staged()` to `defineTable` for validating a schema change in the
  background instead of blocking the deploy on a scan of the table. Pass the
  fields to add or change, set a field to `undefined` to remove it, or pass a
  full validator. The current validator stays enforced while Convex checks
  existing documents and new writes against the staged one. `npx convex deploy`
  and `npx convex deploy --dry-run` report each staged table as validating,
  validated, or failed, and the dry-run warning about scanning large tables now
  suggests staging the validator. Once a staged validator is validated, moving
  it into `defineTable` deploys without scanning the table. A deploy that would
  discard pending or completed staged validation asks for confirmation; pass
  `--allow-unstaging-schema` to skip the prompt in CI. See
  https://docs.convex.dev/database/schemas#staged-validators.
- `ctx.meta.getSnapshotTs()` is now public API. It returns the database
  timestamp of the snapshot the current function reads from, in nanoseconds, on
  the same clock as `db.vars.commitTs`. See
  https://docs.convex.dev/database/advanced/commit-timestamp.
- `convex/server` exports `getServiceUrl`, which returns the base URL of a
  Convex-managed service such as the AI gateway, to pair with `getServiceToken`,
  along with a `ServiceName` type. The client no longer rejects service names it
  doesn't know about, so libraries can adopt new services without requiring a
  newer `convex` version.
- Fixed the Convex MCP server stalling permanently after overlapping tool calls.
- Fixed the error reported when a function or symbol is passed where a Convex
  value is expected. It now names the unsupported type and its path, like
  `Function "[Function]" is not a supported Convex type` at `.cb`, instead of
  throwing `Cannot read properties of undefined (reading 'length')`.
- When a push fails because an environment variable used by `auth.config.ts` is
  not set on the deployment, the CLI now prints the `npx convex env set`
  commands to run and a link to the deployment's environment variable settings.
- Region prompts in the CLI list regions in the same order as the dashboard, and
  `npx convex deployment create --region` accepts the Canada
  (`aws-ca-central-1`) and Asia Pacific (`aws-ap-southeast-2`) regions. See
  https://docs.convex.dev/production/regions.
- `npx convex deployment usage-limits set --metric aiGatewayCostDollars` sets
  the AI gateway spending limit.

## 1.46.0

- Added `.optional()` to all validators. For example, `v.string().optional()` is
  equivalent to `v.optional(v.string())`. This changes TypeScript compatibility
  between validators from different Convex versions; projects sharing validators
  across packages may need to align their `convex` dependency versions.
- `npx convex deploy --dry-run` now warns when a schema change requires checking
  every document in large tables against the new schema, since these deploys can
  take a long time. The warning lists the affected tables along with their
  document counts and sizes.
- Fixed a bug where `npx convex deploy` could fail with a `deployment:data:view`
  permission error when deleting indexes with a deploy key that only has
  `deployment:deploy`. The CLI now reuses the schema evaluation response to size
  the affected tables instead of reading them directly, so
  `deployment:data:view` is no longer required.
- `convex/server` exports `FunctionReference_future`, a reference type whose
  arguments are checked similar to the way Convex checks them at runtime. Use it
  instead of `FunctionReference` when you accept someone else's Convex function
  as a callback and know which arguments you will pass it. Behavior summary: it
  rejects a function requiring arguments you never pass, rejects a function that
  does not declare a top-level argument you do pass, and accepts a function
  taking broader values than you pass. A plain `FunctionReference` gets the
  first and third backwards. The surplus-key check stops at the top level: a
  surplus key inside a nested object, array element, or union arm is not
  detected. A value of the type is usable with `ctx.runMutation`,
  `ctx.scheduler.runAfter`, `createFunctionHandle`, and every other API in this
  package that takes a reference, all of which now accept
  `FunctionReference | FunctionReference_future`. It is deliberately not
  assignable to a plain `FunctionReference`, so forwarding one into code typed
  that way is a compile error rather than a silent loss of argument checking.
  Library code that wants to accept either kind of reference should take that
  same union and read arguments and return types through `FunctionArgs` and
  `FunctionReturnType`, which accept either kind.
- As a part of the `FunctionReference_future` work, the `_args` and
  `_returnType` slots on `FunctionReference` are deprecated. They'll be removed
  in a future version. `FunctionReference` gains an optional `_fn` slot that
  enables the checked comparison of args.

## 1.45.0

- Local deployments now upgrade to a new backend version in place, instead of
  going through a snapshot export and import. Upgrades no longer depend on the
  size of your local data, and no longer prompt about transferring it.
- The Convex MCP server (`npx convex mcp`) now supports the stateless MCP
  protocol (`2026-07-28` MCP specification).
- Added a new `getServiceToken` function that generates service tokens for
  first-party Convex services. This will be used by the upcoming Convex AI
  gateway.
- Improved the error message when `npx convex export` fails because a snapshot
  export is already in progress.

## 1.44.0

- `schema.doc(tableName)` returns the validator for whole documents of a table:
  the table's own validator with the `_id` and `_creationTime` system fields
  added (to each member, for tables defined with a union).
  `schema.id(tableName)` is `v.id(tableName)` restricted to the table names in
  the schema. `docValidator(tableName, tableDefinition)` from `convex/server`
  does the same as `schema.doc` when you only have a table definition.
- The server-provided environment variables `CONVEX_SITE_URL` and
  `CONVEX_CLOUD_URL` are now available on `env` by default in
  `_generated/server.d.ts`, along with other environment variables specified in
  `convex/convex.config.ts`.
- Fixed a bug where the context value provided by `ConvexProviderWithAuth` was
  not referentially stable.
- Fixed a bug where `npx convex login status` would report misleading results
  when the user is logged in but does not have access to the current project.

## 1.43.0

- Added new `npx convex deployment usage` and
  `npx convex deployment usage-limits` CLI commands to view a deployment’s
  current resource usage, and view or edit deployment usage limits.
- When defining hourly cron jobs, you can now omit the `minuteUTC` parameter.
  When omitted, Convex will automatically choose a stable start time within the
  scheduled hour, avoiding concentrating jobs at the top of the hour.
- Mutations can now reference their upcoming
  [commit timestamp](https://docs.convex.dev/database/advanced/commit-timestamp)
  with `ctx.db.vars.commitTs`. While the mutation is running,
  `ctx.db.vars.commitTs` is a placeholder symbol that is replaced with a Int64
  value at commit time that is guaranteed to strictly follow commit order,
  unlike the `_creationTime` system field. This low-level primitive is helpful
  for advanced use cases such as implementing efficient FIFO queues.
- Added a new `v.commitTs()` validator that accepts either the upcoming commit
  timestamp placeholder, or a Int64 commit timestamp.
- The Convex CLI is now able to find the right TypeScript compiler when using
  side-by-side TypeScript 6 and 7 installation. (This setup is used in codebases
  that want to use the native TypeScript 7 compiler but still need access to the
  TypeScript compiler JavaScript API.)
- Deprecated the `typescriptCompiler` parameter in `convex.json`. This parameter
  was only necessary when using the TypeScript Native Preview
  (@typescript/native-preview). With TypeScript 7, Convex automatically picks
  the right binary.
- When a component only uses environment variables that are all optional, it’s
  now possible to omit the `env` argument from `app.use(component, { … })`.
- Fixed a bug where `app.use()` failed to require a second argument when a
  component has required environment variables (e.g.
  `app.use(component, { env: { REQUIRED_VAR: "value" } })`).
- Fixed a bug in `usePaginatedQuery_experimental` where page splits were handled
  incorrectly.

## 1.42.3

- Fixed a bug where the codegen would not sort module paths in an order
  consistent with other platforms when running on Windows. This completes a fix
  that was only partially applied in 1.42.2.

## 1.42.2

- Mutations and actions can now read the raw authentication token used in the
  request by accessing `authToken` in `ctx.meta.getRequestMetadata()`.
- Fixed a circular import in `convex/browser` that caused issues when using the
  `ConvexHttpClient` in some JavaScript environments.
- Fixed a bug in `ConvexProviderWithClerk` that caused the Convex client to
  ignore session changes in some situations.
- Fixed a bug where the codegen would not sort module paths in an order
  consistent with other platforms when running on Windows.
- When running `npx convex dev` outside a Convex project, the CLI now returns an
  error message immediately instead of first asking the user to select a project
  and then failing later.

## 1.42.1

- Fixed an issue where the CLI would be unable to find the `tsgo` binary in
  newer versions of `@typescript/native-preview`.
- Added a new `initialAuthTokenReuse` option to `ConvexReactClient` that
  prevents extra function calls when users re-authenticate.

## 1.42.0

- Added a new `npx convex project create` command that can be used to create new
  projects programmatically.
- Added a new `--names-only` flag to `npx convex env list` (and
  `npx convex env default list`). This flag shows the names of the env vars that
  are set, without the values. It can be useful to let AI coding agents know the
  variables that are set on a deployment, without giving them the actual values.
- Added a new `useStaleSnapshot` option to the arguments for `runQuery`. This is
  an advanced feature that can be used to allow mutations to avoid optimistic
  concurrency control (OCC) conflicts in some cases where they can commit even
  though they depend on conflicting reads. This change allows us to improve the
  performance of some of the official Convex components, including Workpool.
- Improved the documentation of `db.*` methods to more clearly explain the
  difference between the old APIs without table names (e.g. `db.get(userId)`)
  and the new APIs with table names (e.g. `db.get("users", userId)`).
- Fixed an issue where the CLI would not surface permission errors correctly
  when the user or token doesn’t have permission to do something.
- Exposes the current scheduled function's ID as `scheduledFunctionId` in
  `ctx.meta.getRequestMetadata()`.
- `npx convex insights` has a new `--json` flag that makes the command output
  easier to parse programmatically.
- File storage: marked a few TypeScript types in `convex/server` as
  `@deprecated` (`FileMetadata`, `FileStorageId`, `StorageId`). These types are
  used only by file storage APIs that were deprecated in `convex@1.6.0`, so we
  also marked them as `@deprecated` for clarity.
- Bumps the `ws` peer dependency to avoid a vulnerable range.

## 1.41.0

- It is now possible to set limits on nested queries and mutations with the new
  `transactionLimits` option in `runQuery`/`runMutation`.
- `npx convex ai-files` now installs skills with separate copies of each skill
  for each coding agent instead of using symlinks. We made this change to avoid
  known issues with symlinks on Windows.
- When using Convex in anonymous mode (without a Convex account),
  `npx convex dev` now starts a different dashboard server for each deployment.
  This ensures the dashboard always connects to the right deployment when
  multiple deployments are running at the same time.

## 1.40.0

- You can now create a local deployment in a specific Convex cloud project with
  `npx convex deployment create team-slug:project-slug:local`.
- You can now move a local deployment to another cloud project using
  `npx convex deployment select team-slug:project-slug:local`. This command
  warns when it moves the deployment to another project.
- The CLI now shows more clearly which deployment is targeted when running
  commands such as `npx convex dev` and `npx convex deploy`.
- Added a new `<AuthRefreshing />` helper component, used to show indicators
  when function calls are paused because the authentication token is refreshing.
- Removed `--local` and `--cloud` flags from `npx convex dev`. The behavior of
  these flags was misleading when a deployment was already selected. Instead,
  use `npx convex deployment select local` to use a local deployment, and
  `npx convex deployment select dev` to use your personal cloud dev deployment.
- The CLI now provides guidance when TypeScript type checking is taking too
  long.
- Improved the CLI command documentation to include more details and examples.
- `npx convex logs`: `--tail` is now accepted as an alias for the `--history`
  flag.
- When creating a local deployment, the CLI now skips importing the default
  environment variables from the Convex cloud project if you don’t have
  permission to view the default environment variables instead of crashing.

## 1.39.1

- Fixes package.json `bin` entrypoint bug in 1.39.0.

## 1.39.0

- Apps and components may now declare typesafe env vars that they require. Both
  `defineApp` and `defineComponent` now accept an `env` object with string keys
  and validator values. Declared environment variables must be present and match
  the validators before deployment.
- Allow `--local-cloud-port`, `--local-site-port`, `--local-backend-version` and
  `--local-force-upgrade` options to `npx convex dev` to be used whenever using
  a local deployment.
- The `AsyncLocalStorage` and `AsyncResource` APIs from `node:async_hooks` are
  now available in the standard Convex runtime. Note that stored values will not
  be threaded through calls to `ctx.runQuery`/`ctx.runMutation`/`ctx.runAction`.

## 1.38.0

- Adds `ctx.meta.getRequestMetadata()` to access request metadata in mutations
  and actions: request ID, client IP, and client User-Agent.
- Default env vars are now imported when creating a new local deployment.
- Allows creating new local deployments using
  `npx convex deployment select local`.
- Adds support for `--expiry` and `--expires` aliases for `--expiration` in
  `npx convex deployment create`.
- Supports using `import "server-only"` to mark files as server-only.
- Removes the `npx convex disable-local-deployments` command.
- Fixes an issue where stdout/stderr would be truncated after exit in the CLI.
- Updated `usePaginatedQuery_experimental` overload that has an object-based API
  similar to `useQuery_experimental`.

## 1.37.0

- Adds `useQuery_experimental`, which accepts object-based args and returns an
  object with status, data, and error that doesn't throw errors by default.
- Adds `npx convex deployment token create my-token --save-env`, enabling
  scripts to configure agent environments with access only to develop and deploy
  against their own cloud deployment. e.g.

  ```sh
  npx convex deployment create dev/foo --select --expiration "in 7 days" --type dev
  # npx convex env set ... (optional - typically default env vars suffice)
  npx convex deployment token create foo --save-env
  # npx convex dev --once (optional)
  ```

- Re-exports ValidatorTypeToReturnType from convex/server
- AI Files will no longer prompt when connecting to an existing project.
- Updates the `VisibilityProperties` type to allow types like `RegisteredQuery`
  to be more easily extended without losing covariance.
- New `ctx.meta.getDeploymentMetadata()` function.
  - Includes the name, region and class of the deployment.

## 1.36.1

- Updated `npx convex login --vercel` to ask before joining a Convex team
  through the Vercel Marketplace.
- Allows targeting a project or specific deployment type rather than just a
  deployment for `npx convex env default *`.

## 1.36.0

- `npx convex run` now supports `--inline-query` for evaluating readonly queries
  without creating a named function first, for example:
  `npx convex run --inline-query 'await ctx.db.query("messages").take(5)'`.
- Added `ctx.meta` to access extra metadata at runtime:
  - `ctx.meta.getFunctionMetadata()`: the current function's name, component,
    visibility, and whether it's a query, mutation, or action.
  - `ctx.meta.getTransactionMetrics()`: how much capacity remains in the current
    transaction. Only available in queries and mutations.
- Added a `npx convex env default` command for getting and setting default
  environment variables for a project.
- `npx convex deploy` now supports a `--message` option that allows you to write
  details in the deployment’s audit log. When running `npx convex deploy` on CI
  runners on popular platforms (including GitHub Actions, Vercel, and Netlify),
  a default message will be provided if you don’t provide one (e.g. “Deployed
  from GitHub Actions • e83c516”).
- When deploying to a preview deployment, you can now reuse the existing
  deployment instead of creating a new one by using `--preview-name` instead of
  `--preview-create`. This behavior is also used when deploying to preview
  deployments from the CI without specifying `--preview-create` explictly, but
  you can specify `--preview-create` explicitly to restore the old behavior.
  When using the `--preview-run` flag, the function only runs when a new
  deployment is created.
- `npx convex codegen` can now automatically start a local backend when
  necessary.
- You can now increase the timeout used by the CLI when starting a local backend
  with `CONVEX_LOCAL_BACKEND_STARTUP_TIMEOUT_SECS`; this can be useful when
  using large local databases.
- Fixed a bug where `npx convex ai-files remove` would also remove skill files
  that were not installed by `npx convex ai-files`
- Fixed a bug where the automatically-generated files would sometimes sort
  imports in inconsistent order across platforms.
- Fixed a bug in `npx convex dev --start` where, in some cases, the `--start`
  command would continue to run after closing the Convex dev server with
  <kbd>Ctrl+C</kbd>.
- Generating APIs via function-spec now supports validators with int64 literals.

## 1.35.1

- Adds support for automatic preview deployments on Cloudflare Pages
- Removes a new API in `useQuery` that was accidentally included as part of the
  `1.35.0` release.

## 1.35.0

- You can now create and select local deployments explicitly with the CLI:
  - `npx convex deployment create local --select`: initialize local development.
  - `npx convex deployment select dev`: switch to your cloud dev deployment.
  - `npx convex deployment select local`: switch back to your local deployment.
  - To run a single command on your local deployment, use `--deployment local`.
- `npx convex deployment create` now has a `--expiration` flag allowing you to
  create temporary dpeloyments.
- Components can now define HTTP routes in their own `http.ts` file. The routes
  exposed by a component are accessible through a URL prefix that isolates them
  from the other HTTP routes (e.g.
  `app.use(component, { httpPrefix: "/foo" })`).
- `npx convex dev` now supports a new `--start` flag that runs a command in
  parallel with the Convex dev server (e.g. `npx convex dev --start 'vite'`).
  This new option can be used to simplify the `dev` command of most projects.
  This flag was previously called `--run-sh`, and used to not allow long-running
  commands; now, `--run-sh` is an alias of `--start`.
- Defaults non-interactive commands to target an anonymous deployment if one is
  not configured or specified by the deploy key. This enables coding agents to
  more easily develop without needing the CONVEX_AGENT_MODE env variable. By
  default, the CLI will avoid prompting in non-interactive terminals when
  possible.
- You can now develop against your anonymous local deployments even when logged
  in. You will be prompted to link the deployment to a project if the terminal
  is interactive.
- Enables `npx convex deployment create` when you don't yet have a project.
- Improved formatting of sizes in error messages.
- Changes the default codegen to use the component's `ComponentApi` type instead
  of static code generation for components in `_generated/api.d.ts`. To get the
  old behavior, set `{ "codegen": { "legacyComponentApi": true } }` in
  `convex.json`.
- Fixed a bug where `npx convex dev` would not use the correct deployment after
  running `npx convex deployment select`.
- Improved the definition of the `AnyDataModel` type to support using default
  indexes (`by_id`, `by_creation_time`) in a typesafe way.

## 1.34.1

- Hides the `--yes` flag on `npx convex deploy` to discourage YOLO agents being
  run in privileged environments from pushing to production.
- Improves client backoff strategy to reconnect more quickly. The changes in
  1.34.0 did not correctly reset backoff sufficiently after reconnect.
- AI files will not be installed by default in non-interactive environments.
- Disabling AI files is now tracked with an "enabled" flag in convex.json that
  hides nags for initial setup and updates when set to false.
- Fix ai-files re-prompting to install files on setup when they already exist.
- Fix local deployments returning stale credentials when ports change.

## 1.34.0

- The CLI can now
  [automatically provide Convex AI context files](https://docs.convex.dev/ai#convex-ai-files)
  (`AGENTS.md`, `CLAUDE.md`, guidelines/state files) to your project when using
  `npx convex dev`. You can manage Convex AI files using the new
  `npx convex ai-files` command:
  - `npx convex ai-files install` - Install or refresh AI files
  - `npx convex ai-files update` - Update to latest available AI files
  - `npx convex ai-files status` - Show what is installed and what is stale
  - `npx convex ai-files disable` - Suppress install and staleness messages in
    `npx convex dev`
  - `npx convex ai-files enable` - Re-enable install and staleness messages
  - `npx convex ai-files remove` - Remove Convex-managed AI files
- Adds new `npx convex deployment` commands:
  - `npx convex deployment create` allows you to create new cloud deployments
    for a project
  - `npx convex deployment select` allows you to select the deployment to use in
    your project directory when running commands
- CLI commands now support the `--deployment` flag to select a deployment to
  target. It accepts a deployment name (e.g. `joyful-capybara-123`), ref (e.g.
  `dev/james`), `dev` (for your personal dev deployment), or `prod` (for your
  project’s default production deployment). You can also select deployments in
  other projects with `project-slug:ref` or `team-slug:project-slug:ref`.
- Improves websocket client backoff behavior.
- No longer recreates `convex/README.md` when `convex/` already exists.

## 1.33.1

- Fixes the ConvexProviderWithClerk to fetch the JWT template if the (new)
  Convex integration is not enabled. It is safe to both set the JWT template and
  enable the integration. In 1.33.0 it broke if you only had the JWT template
  specified.

## 1.33.0

- `npx convex env set` can now:
  - Accept a value interactively
  - Accept a value via `--from-file`
  - Set multiple variables at once via `--from-file` or stdin, e.g.
    `npx convex env set < .env.defaults`. This will refuse if any existing
    values don't match, unless `--force` is provided. It will not set convex-
    managed variables, such as `CONVEX_DEPLOYMENT` or `VITE_CONVEX_URL`.
  - Auto-start a local backend while setting environment variables if it isn't
    running already.
- `npx convex env list` now emits a more robust format for multi-line and other
  complex environment variables, so you can do:
  `npx convex env list > .env.convex` from one deployment, then
  `npx convex env set < .env.convex` on another. This format is also now
  available on the Dashboard.
- `npx convex run` will auto-start a local backend if necessary, for the
  duration of the call.
- `getConvexSize` will now return 0 for `undefined`, even though it isn't
  stictly a `Value` (it is not serialized for documents, arguments, or results).
- Passing `--env-file` to the CLI works more reliably for specifying the target
  deployment.
- Improves the types of \_generated/api to avoid infinite recursion when you
  have a file and folder with the same name in your convex/ folder.
- Adds `npx convex init` as a command that will go through the interactive
  deployment selection process, if a deployment isn't already configured, but
  not push code. This is useful for `predev` package.json scripts that want to
  do interactive selection steps before running the frontend and backend in
  parallel. It also enables agents to initialize and set environment variables:

  ```
  export CONVEX_AGENT_MODE=anonymous
  npx convex init
  convex env set < .env.defaults
  ```

- No longer overwrites the `tsconfig.json` and `README.md` files in the
  `convex/` folder when configuring a new deployment, only when first creating
  the `convex/` folder.

## 1.32.0

- Improved the API documentation with more examples to help AI agents.

- Added a new `npx convex insights` CLI command to show the insights for a
  deployment.
- Added `insights` MCP tool for diagnosing OCC conflicts and resource limit
  issues on cloud deployments.
- The `insights` MCP tool works on production deployments without requiring
  `--dangerously-enable-production-deployments`.
- When using a local Convex backend (local dev deployment, agent mode or
  anonymous mode), the deployment’s data is now stored in a `.convex` directory
  in the project root (instead of `~/.convex`). This change is helpful when
  using multiple worktrees, since each worktree can get its own isolated
  storage. Existing local deployments are not affected.
- Added new options `maximumRowsRead` and `maximumBytesRead` to
  `PaginationOptions` to get more fine-grained control over the number of rows
  read when using pagination.
- When creating a new dev deployment, the Convex CLI now asks you which
  deployment region you want to use if you haven’t set a team default.
- Increased the default value for `authRefreshTokenLeewaySeconds` to 10 seconds.
- The CLI now uses `VITE_CONVEX_*` environment variables when using Remix
  alongside Vite, instead of `CONVEX_*`.
- Fixed an issue where the CLI would sometimes be affected by GitHub API rate
  limits when downloading the local deployment binary.
- Fixed an issue where websockets would disconnect when using Bun.
- Fixed an issue with the WorkOS integration that caused crashes when running
  `npx convex deploy` with a deployment that has its own WorkOS credentials.
- Fixed an issue with the WorkOS integration where the `WORKOS_API_KEY`
  environment variable from the shell would incorrectly be used.
- Fixed an issue where some modifications to `auth.config.ts` would cause the
  push process to fail.
- Fixed an issue on Windows that caused arrow key presses to be ignored when the
  “cloud or local deployment” prompt is shown.

## 1.31.7

- Add `getConvexSize` and `getDocumentSize` functions to `convex/values` for
  calculating the size of Convex values in bytes. This is the same size
  calculation used for bandwidth tracking and document size limits.
- Optimize code push to only upload changed modules.
- Include `CONVEX_SITE_URL` in environment variables.

## 1.31.6

- Added a new
  [`authKit`](https://docs.convex.dev/production/project-configuration#configuring-workos-authkit-integration)
  field in `convex.json` that allows you to customize the automatic provisioning
  and configuration of WorkOS environments.
- The CLI now warns you when using a Node.js version older than Node.js 20.
- Improved error messages when requests fail in the MCP server.
- Improved the error message when creating a component with an invalid name.

## 1.31.5

- Exclude source code content from bundled sourcemaps by default. This reduces
  the size of the bundled pushed by `npx convex dev` and `npx convex deploy` to
  make them run faster.
- This version drops support for Node.js 18. Please update to Node.js 20 or
  newer.

## 1.31.4

- Fix
  [`window.addEventListener is not a function`](https://github.com/convex/convex-backend/issues/304)
  bug in React Native.

## 1.31.3

- `CONVEX_AGENT_MODE=anonymous` can now be used while logged in.

- The client will try to reconnect immediately after being offline instead of
  waiting for the scheduled backoff delay.

- Optimize bundling during code push, and add `includeSourcesContent` option in
  `convex.json` to configure whether to include source code content in bundled
  sourcemaps.

## 1.31.2

- Bug fix: the TypeScript types of the new `ctx.db` APIs introduced in 1.31.0
  incorrectly allowed passing IDs with types broader than the table name
  argument (e.g. `db.get("table1", id)` where `id` is
  `Id<"table1"> | Id<"table2">`). This issue is fixed in 1.31.2.

## 1.31.1

- You can now increase the speed of type checking when using `npx convex dev` by
  enabling the
  [TypeScript native preview](https://devblogs.microsoft.com/typescript/announcing-typescript-native-previews/)
  ([TypeScript 7](https://devblogs.microsoft.com/typescript/typescript-native-port/)).
  To do so, add `@typescript/native-preview` as a dev dependency and
  [set `typescriptCompiler: "tsgo"` in `convex.json`](https://docs.convex.dev/production/project-configuration#configuring-the-typescript-compiler).

## 1.31.0

- `db.get`, `db.patch`, `db.replace`, and `db.delete` now accept a table name as
  the first argument (e.g. `db.get("messages", messageId)` instead of
  `db.get(messageId)`). This new syntax is more ergonomic, safer, and will allow
  developers to customize IDs in the future. We recommend that all developers
  migrate to the new syntax, using the ESLint rule
  [`@convex-dev/explicit-table-ids`](https://docs.convex.dev/eslint#explicit-table-ids)
  or our standalone codemod tool
  (`npx @convex-dev/codemod@latest explicit-ids`).
  [**Learn more on news.convex.dev**](https://news.convex.dev/db-table-name/)

## 1.30.0

- The `--preview-create` parameter for `npx convex deploy` will now error if
  used with a deploy key that is not a preview deploy key. Previously, the flag
  would be ignored in this situation, and `npx convex deploy` would deploy to
  the production deployment. If you were depending on this behavior, make sure
  to remove the `--preview-create` flag when deploying to production.

## 1.29.3

- Revert ApiFromModules type changes introduced in 1.29.0 which sometimes caused
  type mismatches due to `FunctionReference` sometimes missing properties.

- Don't warn when `"$schema"` is present in convex.json.

- Replace ProxyAgent with EnvHttpProxyAgent in the CLI so the `NO_PROXY`
  environment variable is respected.

## 1.29.2

- When running `npx convex deploy`, the CLI will now ask for explicit
  confirmation before deleting large indexes. This change is helpful for
  avoiding situations where an index is accidentally deleted and backfilling it
  takes a long time.
- You can use `v.nullable` as a convenient alias for `v.union(v.null(), ...)`
  when defining validators.

## 1.29.1

- Support for special error and no-op values of `CONVEX_DEPLOY_KEY` environment
  variable used by the Convex Vercel integration.

## 1.29.0

- Code generation changes: modules and functions are sorted in more situations,
  some unused imports have been removed, and some docstrings have been updated.
  Expect to need to commit a larger-than-usual change to generated files after
  upgrading to this version of Convex.

- Add .pick(), .omit(), .partial(), and .extend() methods to v.objects()
  validators. This makes reusing validator with small changes simpler. See
  <https://docs.convex.dev/functions/validation#reusing-and-extending-validators>
  for more.

- Add a pagination result validation helper
  `paginationResultValidator(itemValidator)` describing and validating the
  return value of a paginated query.

- New `npx convex codegen --component-dir ../path/to/component` flag for
  component authors to generate code only for a component.

- New `convex.json` configuration property `codegen.fileType` (`"dts/js"` or
  `"ts"`, default `"dts/js"`) Default for applications is still "dts/js" but for
  components generated files always use "ts" file extensions.

- New `convex.json` configuration property `codegen.legacyComponentApi` (default
  true) which can be set to false to opt into importing the API of a component
  directly from its package or directory instead of inlining the result of
  analyzing a component in parent component that uses it.

- Improved TypeScript inference performance for `ApiFromModules`, the workhorse
  type that transforms modules of Convex functions into a tree of
  `FunctionReference` types for the `api` object. Thanks to David Blass, the
  maintainer of ArkType, for working with us on these improvements.

## 1.28.2

- Bundling fix: don't double-deploy components in the convex/ directory.

## 1.28.1

- Add json schema to package.json.

## 1.28.0

- Deploy code path unification: all deploys now use a codepath that supports
  components, whether or not any components are used in the project. Generating
  the files in `convex/_generated/` now requires a deployment to be present and
  for all environment variables used in convex/auth.config.ts to be set.

  Scripts that call `npx convex codegen` are the most likely to be affected by
  this change, and `npx convex codgen` no longer works for any projects in
  preview deployments because preview deployments may not exist until the
  deploy.

  Committing generated code is recommended and this change makes this
  recommendation more important.

- WebSocket sync protocol support for TransitionChunk messages: just splitting a
  Transition (containing new query results) into multiple WebSocket messages in
  order prevent the server from appearing non-responsive.

## 1.27.5

- Export an `AuthConfig` type to describe the object exported from
  `convex/auth.config.ts`.

## 1.27.4

- Add a `getAuth()` method to the client which returns the current token and
  claims. This method is intended for instrumentation purposes like adding this
  information to a reported Sentry error or event.

- Change to CLI `--admin-key` and `--url` arg parsing logic to avoid coercing
  empty strings to booleans.

- Vendor jwt-decode along with a few other dependnecies; this brings the number
  of runtime dependencies for Convex from 3 to 2: esbuild (binary) and prettier.

- Fix ConvexProviderWithClerk to catch `getToken()` errors. This could cause
  changes in behavior of refreshing Clerk tokens, we'll be watching this one.

## 1.27.3

- Convex CLI now respects `HTTPS_PROXY` / `HTTP_PROXY` environment variables.
  This is generally useful but was motivated by making Convex run in remote
  asynchronous agent environemnts like Codex.
- Cutoffs for logging large and slow Transition messages have been lowered.

## 1.27.2

- `npx convex run` sends function log output to stderr instead of stdout.

- Increase the timeout when waiting for a local backend to start up from 10 to
  30 seconds.

- Capability to provision WorkOS environments when `WORKOS_CLIENT_ID`
  environment variable is missing.

## 1.27.1

- Changes to logged messages in the console, where previously only WebSocket
  reconnection messages were logged. These can still be silenced with the
  `logger: false` client option.
- Improved support for the TypeScript tsconfig.json compiler option
  "exactOptionalPropertTypes" with the convex package.
- Additional WebSocket connection debugging information about Transition
  messages.
- Clearer error messages when running codegen in preview deployments.
- Token limits for especially verbose logs MCP tool.
- Add (preexisting) `codegen` options to `convex.json` schema.

## 1.27.0

- Add support for configuring the Node.js version used by Node actions using the
  `node.nodeVersion` field of the `convex.json` file
  ([docs](https://docs.convex.dev/production/project-configuration#configuring-the-nodejs-version)).
- `npx convex data` now supports exporting data as JSON or JSONL with the
  `--format` option.
- `npx convex env set` now supports setting environment variables via piping
  (STDIN) (for example
  `cat keys/my-private-key.txt | npx convex env set JWT_PRIVATE_KEY`).

## 1.26.2

- Fix for pushing schemas to older (self-hosted) deployment builds.

## 1.26.0

- Add support for staged indexes. Instead of blocking on push, a staged index
  backfills while you push more changes to a deployment, then can be used once
  it is no longer marked as staged.

  ```
  export default defineSchema({
    messages: defineTable({
      author: v.string(),
      body: v.string(),
    })
      .index("by_author", {
        fields: ["author"],
         // watch for progress in dashboard. Once it's at 100%, remove the staged flag
        staged: true,
      })
  });
  ```

  Read more in the docs (link coming soon).

- Experimental `ConvexReactClient.prewarmQuery({query, args})` method for
  subscribing to a query for 5 seconds. Prewarming indicates likely future
  interest in a subscription and is currently implemented by subscribing to the
  query for 5 seconds.

  The return value of this method may change and the arguments may change in the
  future so this API should be considered unstable but adapting to these changes
  shouldn't be difficult.

- Expose the schemaValidation property of schema, intended for runtime tests or
  assertions that it is indeed enabled.

- Export TokenFetcher type, intended for external auth integrations.

- More informative messaging on code push. Link to index backfill progress in
  the dashboard.

- Fix a bug where an auth token passed initially to the ConvexHttpClient was
  ignored.

- Experimental `expectAuth` option for Convex clients for indicating that
  setAuth() will be called soon, so to wait for that token. Once setAuth() has
  been called the existing token-waiting behavior takes over.

  This is useful for applications that create a client and run a mutation,
  query, or action _before_ setAuth() has been called, e.g. via a provider in
  React.

- Change the default permissions for the local MCP server: access to production
  deployments is now disabled by default, requiring
  `--dangerously-enable-production-deployments` to enable.

- Add a "logs" tool to the local MCP server.

## 1.25.4

- Experimental `convex dev --once --debug-node-apis` debug flag for tracing
  through imports to find Node.js APIs imported from non-"use node" convex
  endpoint files.

- Experimental `CONVEX_AGENT_MODE=anonymous npx convex dev` for a
  non-interactive anonymous development flow when not logged in.

- Simplify the `Logger` interface of Convex clients making it easier to replace.

- Reactive `ConnectionState` via `useConvexConnectionState` hook. How often this
  value updates may change in the future if more information is added to
  `ConnectionState`.

- Escape JavaScript keywords in codegen, fixing issues with files in the convex
  folder named things like convex/delete.ts.

- Warn when the explicit value `undefined` is passed to Next.js server-side
  helpers, indicating the issue may have been an unset environment variable.
  This may become an error in the future.

- Error closer to the call site when invalid validators are contructed.

## 1.25.2

- Increase a network timeout that was causing Node.js v20+ issues on slow
  connections, good old Happy Eyeballs
  <https://github.com/nodejs/node/issues/54359>.

## 1.25.1

- Print more error info in `npx convex network-test`.

- Crash when it looks like an environment variable is missing:
  `CONVEX_DEPLOY_KEY=project:me:new-project|eyABCD0= npx convex` parses as
  `CONVEX_DEPLOY_KEY=project:me:new-project | eyABCD0='' npx convex` but when
  was meant was
  `CONVEX_DEPLOY_KEY='project:me:new-project|eyABCD0=' npx convex`.

  Crash when an environment variable like `ey...0=` is present to surface errors
  like this quicker.

## 1.25.0

To upgrade to this release you'll need to upgrade any Convex components you use.

- Set `process.env.NODE_ENV = "production"` during Convex function bundling.
  This will result in different code being bundled from some packages, generally
  faster code.

- Smaller bundles via esbuild minification.

- ConvexHttpClient mutations are now queued by default, making the
  ConvexHttpClient match the behavior of ConvexClient and ConvexReactClient.
  This makes switching between these safer.

  If you need unqueued mutations (you need to run multiple mutations
  concurrently), pass the unqueued: true option or create a separate
  ConvexHttpClient for each queue of mutations you need.

- Allow passing auth to ConvexHttpClient as an option in the constructor. This
  is appropriate for short-lived ConvexHttpClients and it more convenient for
  instantiating a client and using it in a single expression.

- Restore check that Convex functions are not imported in the browser.

  Convex functions run in a Convex deployment; including their source in a
  frontend bundle is never necessary and can unintentionally reveal
  implementation details (and even hardcoded secrets).

  This check current causes a `console.warn()` warning, but in future versions
  this will become an error. If you see the warning "Convex functions should not
  be imported in the browser" you should address this by investigating where
  this is being logged from; that's code you don't want in your frontend bundle.
  If you really want your Convex functions in the browser it's possible to
  disable this warning but this is not recommended.

- TypeScript error when async callbacks are passed to
  `mutation.withOptimisticUpdate()`: an optimistic update function is expected
  to run synchronously.

- Experimental onServerDisconnectError() callback option for Convex clients.

  This is a stopgap client callback to report unusual server disconnect errors.
  The content of these messages, which messages trigger the callback, and the
  existence of the callback are all subject to change as we develop better
  interfaces for disconnect metrics.

## 1.24.8

- Restore short retry timer for WebSocket reconnects initiated by an error on
  the client. This behavior was inadvertently changed in 1.24.7.

## 1.24.7

- Increase WebSocket client timeouts in general and especially for abnormal
  server errors. See
  [this incident postmortem](https://news.convex.dev/how-convex-took-down-t3-chat-june-1-2025-postmortem/)
  for more context.

## 1.24.6

- Fix another bug with new Custom JWT auth support in projects that use Convex
  backend components.

## 1.24.5

- `ConvexClient.mutation()` now accepts a third `options` argument that can
  contain an optimistic update.

## 1.24.3

- Add `.url` property to ConvexReactClient.

- Earlier errors when invalid objects are passed to `defineTable()`.

## 1.24.2

- Fix bug with new Custom JWT auth support in projects that use Convex backend
  components.

- Support larger data imports from the command line by choosing larger chunk
  size when necessary.

- Calling setAuth on a disabled ConvexClient is now an no-op.

- Add `npx convex dash` alias for dashboard command.

- Limit the number of nested query operators to 256.

## 1.24.1

- Accept `true` and `false` as values for logger in all clients, making
  disabling logs from convex functions run on a development deployment simpler:
  it's no longer necessary write your own no-op logger.

## 1.24.0

- Drop support for React 17 and remove use of `unstable_batchedUpdates` as React
  18 introduced
  [Automatic batching](https://react.dev/blog/2022/03/29/react-v18#new-feature-automatic-batching)

  If you use React 17 and choose to override the change in supported peer
  dependencies (please don't), you may notice query updates are no longer
  batched: it's possible for one Convex query update to occur on a different
  React render than another causing single frames of discrepancies in UI or
  worse, errors if you have code that relies on the client-side consistency like
  client-side joins.

  You may also notice nothing. Without batched updates some queries may be a few
  milliseconds ahead of other queries, which is still much less than the
  differences in other data fetching solutions, e.g. React Query or SWR, in
  non-batched mode.

- Remove dependency on `react-dom`, making it possible to use on "React Native
  only" projects without overriding any dependency resolution.

- New optimistic update helpers for paginated queries: three helpers
  `insertAtTop`, `insertAtBottomIfLoaded`, and `insertAtPosition`.

- The `npx convex login --login-flow paste` flag can be used to explicitly opt
  into the manual token paste login method.

- Fix MCP servers for self-hosted deployments: previously MCP CLI commands were
  attempting to contact a cloud deployment (which didn't exist) in self-hosted
  setups.

- New `compareValues` function exported from `convex/values` which matches
  Convex values semantics as implemented in backends. This function should match
  the Rust implementation in backend (and it property-tested in pursuit of
  this!) but in the event of discrepancies the Rust implementation should be
  considered authoritative.

## 1.23.0

- `npx convex dev` now supports the option of running Convex locally without an
  account

## 1.22.0

- Options for turning off MCP tools and blocking prod deployments (see
  `npx convex mcp`)
- Add `--run-sh` option to `npx convex dev`, similar to `--run` but for shell
  commands
- Add `inflightMutations` and `inflightActions` to
  `convexClient.connectionState()`

## 1.21.0

- `npx convex dev` tails logs by default. See the `--tail-logs` option for more.

- Improvement to the `.unique()` error message to print `_id`s
  [PR](https://github.com/convex/convex-backend/pull/59)

- Fixes to avoid race conditions in auth
  [PR](https://github.com/convex/convex-js/pull/29)

## 1.20.0

- Calling registered functions directly like helper functions no longer
  typechecks. See release notes for 1.18.0 for more.

- Upgrade esbuild for a sourcemap bug fix.

- Fix FieldTypeFromFieldPath to handle union of nested values and primitives.

## 1.19.5

- `npx convex mcp start` runs an MCP server. AI agents can introspect deployment
  schema (both declared and inferred) and function APIs, read data from tables,
  call functions,and write oneoff queries in JS.

## 1.19.3

- Upgrade esbuild from 0.23 to 0.25 to address security warnings about
  <https://github.com/evanw/esbuild/security/advisories/GHSA-67mh-4wv8-2f99>

  Convex does not use the development server functionality of esbuild which
  contains the vulnerability.

## 1.19.2

- Improved support for working with self-hosted deployments: every command that
  makes sense (e.g. not `npx convex login`) works with self-hosted deployments.

  The environment variables `CONVEX_SELF_HOSTED_URL` and
  `CONVEX_SELF_HOSTED_ADMIN_KEY` are now used to identity self-hosted
  deployments.
  <https://github.com/convex/convex-backend/tree/main/self-hosted#self-hosting-convex>
  for more.

- export the `ValidatorJSON` record types.

## 1.19.0

- Support for Local Deployments, now in beta. See
  <https://docs.convex.dev/cli/local-deployments> for more.

  Local deployments run your Convex dev deployment for your project on your
  local machine, which should make syncing your code faster. It also makes
  resources used during development like function calls and database bandwidth
  free, since it's your own compute resources you're using!

## 1.18.0

- Warn on direct Convex function call. This adds a console.warn whenever a
  Convex Function (mutation, query, action, internalMutation, etc.) is called
  directly

  ```ts
  export const foo = mutation(...);

  export const bar = mutation({
    args: v.any(),
    returns: v.any(),
    handler: (ctx, args) => {
      const result = await foo();
    })
  }
  ```

  because this pattern causes problems and there are straightforward
  workarounds. The problems here:
  1. Arguments and return values aren't validated despite the presence of
     validators at the function definition site.
  2. Functions called this way unexpectedly lack isolation and atomicity. Convex
     functions may be writting assuming they will run as independent
     transactions, but running these function directly breaks that assumption.
  3. Running Convex functions defined by customFunctions like triggers can cause
     deadlocks and other bad behavior.

  There are two options for how to modify your code to address the warning:
  1. Refactor it out as a helper function, then call that helper function
     directly.
  2. Use `ctx.runMutation`, `ctx.runQuery`, or `ctx.runAction()` instead of
     calling the function directly. This has more overhead (it's slower) but you
     gain isolation and atomicity because it runs as a subtransaction.

  See
  <https://docs.convex.dev/understanding/best-practices/#use-helper-functions-to-write-shared-code>
  for more.

  Filter to warnings in the convex dashboard logs to see if you're using this
  pattern.

  For now running functions this way only logs a warning, but this pattern is
  now deprecated and may be deleted in a future version.

- Support for Next.js 15 and
  [Clerk core 2](https://clerk.com/docs/upgrade-guides/core-2/overview):
  `@clerk/nextjs@5` and `@clerk/nextjs@6` are now known to work to Convex. Docs,
  quickstarts and templates have not yet been updated. If you're upgrading
  `@clerk/nextjs` from v4 or v5 be sure to follow the Clerk upgrade guides as
  there are many breaking changes.

- Improvements to `npx convex run`:
  - Better argument parsing with JSON5 so `{ name: "sshader" }` parses
  - support for `--identity` similar to dashboard "acting as user" feature, like
    `npx convex run --identity '{ name: "sshader" }'`
  - `npx convex run api.foo.bar` is equivalent to `npx convex run foo:bar`
  - `npx convex run convex/foo.ts:bar` is equivalent to `npx convex run foo:bar`
  - `npx convex run convex/foo.ts` is equivalent to `npx convex run foo:default`

- Allow non-JavaScript/TypeScript files in the `convex/` directory. Only .js
  etc. files will be bundled and may define Convex functions points but adding a
  temporary file like `convex/foo.tmp` will no longer break` the build.

- Fix type for FieldTypeFromFieldPath with optional objects.

- Fix types when a handler returns a promise when using return value validators
  with object syntax.

## 1.17.4

- Revert use of the identity of useAuth from Clerk to determine whether
  refreshing auth is necessary. This was causing an auth loop in Expo.

## 1.17.3

- Fetch a new JWT from Clerk when using Clerk components to change the active
  orgId or orgRole in React on the client. Any auth provider can implement this
  by returning a new `getToken` function from the `useAuth` hook passed into
  `ConvexProviderWithAuth`.

## 1.17.2

- Revert local Prettier settings change described in 1.17.1 which removed angle
  brackets in some cases where local prettier config used plugins.

- `npx convex import --replace-all` flag which behaves like the Restore
  functionality in the dashboard.

## 1.17.1

- Use local Prettier settings to format code in `convex/_generated` if found.
- Extend supported react and react-dom peerDependencies to include v19
  prereleases. This is temporary, only stable React 19 releases will be
  supported in the long term.
- Hook up Sentry reporting for local deployments, opted-into by
  `npx convex dev --local`. This telemetry will be made configurable before this
  feature is released more broadly. This is being called out here for
  transparency regarding telemetry, but this `--local` feature is not yet ready
  for general consumption. Please don't use it unless you're excited to help
  test unfinished features and willing to have errors submitted to Convex.
- Don't try to bundle .txt or .md files in the convex/ directory.
- Don't include credentials in HTTP client requests.

## 1.17.0

- Disallow extra arguments to CLI commands.
- `--component` flags for `convex import` and `convex data`.
- `--run-component` flag for `convex dev --run`
- Remove prettier-ignore-start directives from generated code.
- Fix file watcher bug where a syntax error could cause a file to stop being
  watched.
- Downgrade jwt-decode dependency back to ^3.1.2.
- Change refresh token renewal timing

## 1.16.6

- Detect TanStack Start projects and use environment variable name
  `VITE_CONVEX_URL`.

## 1.16.5

- restore `--run` flag of `convex import`

## 1.16.4

- Don't typecheck dependent components by default, add `--typecheckComponents`
  flag to typecheck.

## 1.16.3

- Fix some library typecheck errors introduced in 1.16.1. Workaround for
  previous versions is to add `"skipLibCheck": true` to the tsconfig.json in the
  convex directory.

## 1.16.2

- Change some language around components beta.

## 1.16.1

- Release components, a feature in beta. These codepaths should not be active
  unless a convex directory contains a file named `convex.config.ts`. Components
  aren't documented yet and may change significantly before general release.

## 1.16.0

- Added support for a new validator, `v.record`. This is a typed key-value
  object in TypeScript. More information can be found in the
  [docs](https://docs.convex.dev/functions/validation#record-objects).
- Upgrade esbuild from 0.17 to 0.23. It's possible to use an npm override to use
  a different version of esbuild if you need to stay on an older version,
  although changes to the esbuild API could break this in the future.

  See
  [esbuild changelog](https://github.com/evanw/esbuild/blob/main/CHANGELOG.md)
  for the full list of changes. One standout: tsconfig.json is no longer used by
  esbuild for `jsx` setting. Convex now sets it manually to
  ["automatic"](https://esbuild.github.io/api/#jsx).

## 1.15.0

- Added new command, `npx convex function-spec`, that exposes the function
  metadata (name, type, validators, visibility) of functions defined in your
  Convex deployment
- Generated code no longer includes the "Generated by convex@version" comment
- Fix issue with `convexClient.query()` so it always returns a Promise

## 1.14.0

- Updates to ConvexReactClient to work better with authentication and server
  rendering
- `npx convex init` and `npx convex reinit` have been deprecated in favor of
  `npx convex dev --configure`
- Drop support for Node.js v16, and with it drop the dependency on node-fetch.
  This removes the 'punycode' deprecation warning printed when running the CLI
  in more recent versions of Node.js.
- Support for custom claims in JWTs

## 1.13.2

- Fix `npx convex import` regression in 1.13.1

## 1.13.1

- Relax client URL validation to prepare for Convex backends accessible on
  arbitrary domain. This makes `skipConvexDeploymentUrlCheck` client option also
  no longer required for accessing deployments not hosted on the Convex BaaS.

- Fix bug where the first mutation request send over the WebSocket failing would
  not roll back the corresponding optimistic update (completedMutationId could
  be 0 which is falsey!)

- Fix bug where `codegen --init` would fail if no Convex directory existed yet.

- Action and query function wrappers now also allow validators for args
  (previously only objects were accepted) and objects for returns (previously
  only validators were accepted).

- Change `httpRouter` behavior for overlapping paths: exact matches first, then
  the longest prefix patch that matches.

## 1.13.0

- Convex queries, mutations, and actions now accept `returns:` property to
  specify a return value validator.

  Return value validators throw a runtime error (so will roll back the
  transaction in a mutation) when the value returned from a query or mutation
  does not match this validator. This is _stricter than TypeScript_ in that
  extra properties will not be allowed.

- Validator fields are now exposed: the return value of `v.object({ ... })` now
  has a `.fields` property with the validators for each property on it.

  ```
  const message = v.object({ user: v.string(), body: v.string() });
  const imageMessage = v.object({ ...message.fields, })
  ```

  These validators are also exposed on the schema at
  `schema.tables.messages.validator`

  The `Validator` export is no longer a class. It is now a discriminated union
  type of all validators where the `.kind` as the discriminator. The `Validator`
  type still has three type parameters with only the first (the TypeScript type
  enforced by the validator) required.

  The second type parameter IsOptional is no longer a boolean, is it "optional"
  or "required" now.

  These are breaking changes if you're using the two optional type parameters of
  `Validator` or doing `instanceof` checks with `Validator`! We apologize for
  the inconvenience. The same users this affects should be the ones that most
  benefit from the ability to work with validator types more directly.

- Argument validators now accept validators (object validators and union
  validators) in addition to objects with validators as properties. Return value
  validators similarly accept either validators or objects with validators as
  properties, but unlike `args:` any validator is allowed.

  Custom function wrappers (AKA middleware) should continue to work, but to
  present the same API has the builtin Convex function wrappers `query`,
  `internalQuery`, `mutation` etc. you'll need to update such code to accept
  either a validator or an object of validators. You'll likely want to update
  these anyway to support return value validators. The new `asValidator` helper
  maybe useful here.

- The default tsconfig.json created in projects when first creating the
  `convex/` directory now uses `"moduleResolution": "Bundler"`. This is just a
  better default, you
  [probably never want the previous default `"node"`/`"node10"`](https://www.typescriptlang.org/tsconfig/#moduleResolution).

## 1.12.1

- Fix bug where `npx convex deploy` and `npx convex dev` would incorrectly skip
  pushing if the only change was removing files

## 1.12.0

- `npx convex env set` works with `ENV_VAR_NAME=value` syntax

## 1.11.3

- Fix bug when filling out an empty env file
- Exclude files beginning with # from convex directory entry points
- Warn when pushing with an https.ts file
- throw if argument to Query.take() is not an integer

## 1.11.2

- Fix timestamps in npm convex logs

## 1.11.1

- Allow Clerk 5 (currently in beta) in convex peerDependencies
- Fix typechecking bug on Windows caused by the Node.js patch for CVE-2024-27980
  that makes running tsc.CMD directly no longer work
- Exclude jsonl from convex directory entry points
- Add autocomplete for project selection in new project flow
- output debugBundlePath as full bundle instead of as a single file

---

Find release notes for versions before 1.11.3 on the
[Convex Blog](https://news.convex.dev/tag/releases/).
