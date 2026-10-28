# Testing component

This package exists to test components when a different copy of convex is
installed, so uses `"injected": true` for the convex dependency.

This is no longer the canonical Convex component. See
https://github.com/convex/ratelimiter-component for the actual rate limiter
components or https://github.com/convex/templates/tree/main/template-component
for the template that powers `create-convex`:

```
npx create-convex@latest --component
```

# Structure of a Convex Component

Components are expected to expose the entry point convex.config.js. The on-disk
location of this file must be a directory containing implementation files. These
files should be compiled to ESM, not CommonJS.

The package.json should contain `"type": "module"` and the tsconfig.json should
contain `"moduleResolution": "Bundler"` or Node16 in order to import other
component definitions.

In addition to convex.config.js, a component may expose other exports.

A client that wraps communication with the component for use in the Convex
environment is typically exposed as a named export `MyComponentClient` or
`MyComponent` imported from the root package.

```
import { MyComponentClient } from "my-convex-component";
```

Frontend code is typically published at a subpath:

```
import { FrontendReactComponent } from "my-convex-component/react";
```

Frontend code should be compiled as CommonJS code as well as ESM and make use of
subpackage stubs (see next section).

If you do include frontend components, prefer peer dependencies to avoid using
more than one version of e.g. React.

### Support for Node10 module resolution

The [Metro](https://reactnative.dev/docs/metro) bundler for React Native
requires setting
[`resolver.unstable_enablePackageExports`](https://metrobundler.dev/docs/package-exports/)
in order to import code that lives in `dist/esm/frontend.js` from a path like
`my-convex-component/frontend`.

Authors of Convex component that provide frontend components are encouraged to
support these legacy "Node10-style" module resolution algorithms by generating
stub directories with special pre- and post-pack scripts.
