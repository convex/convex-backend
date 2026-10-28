# npm dependency notes

The version consistency check (`just check-js-versions`) pushes us toward using
a single version of each dependency in the monorepo. Our version choices are
compromises between the competing needs of

- staying up to date enough to cutting edge to be able to pull in security
  updates
- staying up to date because we like nice things
- shipping very compatible client code
- using old AND new versions in demos and tests that are representative of
  customer use cases.

We can use multiple versions by adding the less common one to
allowedAlternativeVersions in npm-packages/common-versions.json (checked by
`just check-js-versions`).

# node version

Node.js 20 is the expected version for the whole repo. We no longer run
integration tests with Node.js 18.

We're rolling out configurable Node.js versions for Lambda functions ("use
node") and function runner / local backend now expect Node.js 18, 20, or 22.

# JS build systems

npm-packages/convex is built with custom esbuild + tsc scripts. zshy looks
great, we've just moved a few simple packages over but we may be able to use it
for npm-packages/convex.

# Dependencies that are hard to upgrade

Run `just pnpm update -r` from npm-packages to upgrade within semver specs. If
this doesn't work we need to narrow our semver requirement spec for that
library.

Run `just pnpm update -r --latest --interactive` to upgrade libraries beyond
their current semver spec. See notes below for these libraries.

### react and react-dom

The `react` peerDependency spans 18 and 19 (React 17 is no longer supported).
The `react` devDependency is on 19, so the emitted `.d.ts` files are checked
against the React 19 types only; anything React-18-incompatible has to be caught
by the React 18 workspace demos rather than by convex's own type check.

### TypeScript

We should be more aggressive here. Currently we require 5.0.3 as a minimum. We
can go up from here.

### jwt-decode

This is a unbundled dep, we had issues when we upgraded.
https://github.com/convex/convex/pull/30674

### Chalk

We want to stay on version 4 of chalk forever (well until ship the CLI as CJS,
version 5 is an esm-only build but we want to need to bundle CJS into our CLI.

### openid-client

It's a rewrite, will need to test carefully.

### Inquirer

Just haven't gotten to it, breaking changes are probably simple

### Commander

dunno

### Vitest 1->2

Big changes

### esbuild

macOS 10.15 Catalina no longer supported in esbuild@0.24.0, maybe we care

### typedoc

A bit of custom work to do, we have patched versions of some libraries in docs
that depend on this. Maybe this means we can get rid of them!

### sentry/node 8

esm something or other

### abortcontroller-polyfill

1.7.7
(https://github.com/mo/abortcontroller-polyfill/commit/575383ecb91a0f77a571b59e9c4e223832f032d9
maybe?) breaks something
