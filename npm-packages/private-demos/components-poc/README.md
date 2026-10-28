# Components example

Run a local backend with components enabled:

```bash
just run-local-backend
```

Run `convex dev` against the local backend (be sure to `just turbo run build` if
needed):

```bash
just convex dev
```

Try running the demo function:

```bash
just convex run messages:componentTest
```

# In progress

### Bundling

Definitions work but no explicit functions are being sent up so only implicit
exports are working really.

Some confusion around terms ComponentPath, DefinitionPath, etc.

### Authoring a component

When authoring a component a developer needs to

- run `npx component create somewhere/myComponent`

### Codegen

Components need to generate

1. `_generated/server.ts` for its function wrappers like mutation() generated
   specifically for

   - the parameters of the component definition, (use analysis)
   - the component definition's schema (could be a type import?)
   - the component definitions's child components (use analysis)

2. `_generated/api.ts` for functions to call and schedule other functions. This
   could just be the old lazy api since this isn't imported in the component
   definition.

3. `_generated/component.ts` for the types for `defineComponent` that wil be
   used by componentDefinitions? This will be an autocompletion stumbling block
   no worse than for implementations.

None of these are implemented yet.

For codegen we need a notion of "these components are part of this project; we
are responsible for runnign codegen for them.

When importing a component definition from another package we should consider it
"frozen;' it's read-only, and when we analyze it, we don't need to generate any
code to "fix" the types.

# TODOs removed from public code

Play with npm packaging

> // TODO It will be important when developers write npm package components //
> for these to use a specific entry point or specific file name? // I guess we
> can read the files from the filesystem ourselves! // If developers want to
> import the component definition directly // from somewhere else,

> is the `const candidates = [args.path]` logic really necessary?

Allow transistive imports of component definitions

> // TODO compress these inputs so that everything that is not a
> convex.config.ts has its dependencies compacted down. // Until then we just
> let these missing links slip through.

> I'd like to move all the bundling logic into the existing bundler code // TODO
> These should all come from ../../../bundler/index.js, at least helpers.

And besides, the error reporting code is duplicated

> // TODO In theory we should get exactly the same errors here as we did the
> first time. Do something about that. // TODO abstract out this esbuild wrapper
> stuff now that it's in two places

Node.js bundles

> // TODO Node compilation (how will Lambdas even work?) const \_nodeResult = {
> bundles: [], externalDependencies: new Map(), bundledModuleNames: new Set(),
> };

registerEsbuildReads needs to be stretched out between the two esbuild
invocations! Changes to the component definition dep tree will not be picked up
or invalidated.

in components.ts

> // TODO // Note we need to restart this whole process if any of these files
> change! // We should track these separately if we can: if any thing observed
> // by the definition traversal changes we need to restart at an earlier //
> point than if something in one of the implementations changes. // If one of
> the implementions changes we should be able to just rebuild that.

Gather push dependencies without doing an inadvertent bundle

> // Note that it bundles!!! That's a step we don't need. const { config:
> localConfig } = await configFromProjectConfig(
