# TypeScript monorepo

## Development workflow

When doing changes in `/npm-packages/<package>`:

```sh
# After each modification
just format-js

# When the change is ready
just lint-js
just turbo run build --filter=<package>...

# To run a specific test file
cd npm-packages/<package>/
npm run test -- <file>
```

## Dependencies management

This project uses pnpm workspaces to manage dependencies.

After modifying the dependencies of a package, run `just update-js`.

## Code organization

The public docs live at https://docs.convex.dev/ and the Convex Cloud dashboard
at https://dashboard.convex.dev/.
