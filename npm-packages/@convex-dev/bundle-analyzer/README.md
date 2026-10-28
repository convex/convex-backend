# @convex-dev/bundle-analyzer

<!-- prettier-ignore -->
> [!NOTE]
> This package is in alpha. Commands and output shapes may change between
> releases.

Analyze your Convex project to identify ways to optimize your bundle size.

Run the analyzer from the root directory of your project:

```sh
npx @convex-dev/bundle-analyzer summary
```

## Commands

Start with `summary` or `insights`, then drill into whatever stands out with
`packages`, `files`, or `modules`.

### `summary`

An overview of your bundle: its total size, the top insights, and its largest
components, packages and modules.

```sh
npx @convex-dev/bundle-analyzer summary
```

### `insights`

Suggestions for making your bundle smaller.

```sh
npx @convex-dev/bundle-analyzer insights
```

### `components`

The size of each component's bundle for each runtime. Use it to see which
component to look at first.

```sh
npx @convex-dev/bundle-analyzer components
```

### `packages`

The largest npm packages across your bundles. Pass a package to see which of
your entry points import it.

```sh
npx @convex-dev/bundle-analyzer packages
npx @convex-dev/bundle-analyzer packages zod
```

### `files`

The largest individual files across your bundles. Pass a file path, or part of
one, such as `zod@3.25.76/` for a package's files, to see which modules it's
bundled into and what imports it.

```sh
npx @convex-dev/bundle-analyzer files
npx @convex-dev/bundle-analyzer files convex/lib/
```

### `modules`

How much code the runtime loads to run a function in each module: the module and
the shared chunks it imports. Pass a module or chunk to see the files in it,
what it imports, and which modules import it.

```sh
npx @convex-dev/bundle-analyzer modules
npx @convex-dev/bundle-analyzer modules convex/messages.ts
```

### `explore`

Browse your bundle in an interactive treemap in the terminal. Total size
includes code, source maps, and external packages estimated from your local
node_modules, matching `summary`.

```sh
npx @convex-dev/bundle-analyzer explore
```

Run any command with `--help` to see its options. `--limit` sets how many rows
each list shows. In `files` and `packages` details, it also limits entry points
per copy. Those commands default to 30 list entries and 3 import chains per
copy.

### JSON output

Every command except `explore` takes `--json` to print JSON to stdout instead of
a report, for scripts and AI agents. Insights expose `savedCodeBytes` and
`savedSourceMapBytes`; `savedBytes` is their combined estimated saving.

```sh
npx @convex-dev/bundle-analyzer packages zod --json
```

## Requirements

- `convex/_generated/` must already exist. Run `npx convex dev --once` or
  `npx convex codegen` first. The analyzer doesn't generate code.
- The project's dependencies must be installed, since bundling resolves its
  imports from `node_modules`.

## Version matching

The analyzer will warn if the project's `convex` version differs from the
analyzer's version of `convex`. There may be slight differences in the way the
bundler behaves in different `convex` versions. A planned future feature is to
support analyzing an existing bundle, independent of `convex` version.
