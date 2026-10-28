# Packaged Convex Components

Read this file when the user wants a reusable npm package or a component shared across multiple apps.

## When to Choose This

- The user wants to publish the component
- The user wants a stable reusable package boundary
- The component will be shared across multiple apps or teams

## Default Approach

- Prefer starting from `npx create-convex@latest --component` when possible
- Keep the official authoring docs as the source of truth for package layout and exports
- Validate the bundled package through an example app, not just the source files

## Build Flow

When building a packaged component, make sure the bundled output exists before the example app tries to consume it.

Recommended order:

1. `npx convex codegen --component-dir ./path/to/component`
2. Run the package build command
3. Run `npx convex dev --typecheck-components` in the example app

Do not assume normal app codegen is enough for packaged component workflows.

## Package Exports

If publishing to npm, make sure the package exposes the entry points apps need:

- package root for client helpers, types, or classes
- `./convex.config.js` for installing the component
- `./_generated/component.js` for the app-facing `ComponentApi` type
- `./test` for testing helpers when applicable

## Testing

- Use `convex-test` for component logic
- Register the component schema and modules with the test instance
- Test app-side wrapper code from an example app that installs the package
- Export a small helper from `./test` if consumers need easy test registration

## Checklist

- [ ] Packaging is actually required
- [ ] Build order avoids bundle and codegen races
- [ ] Package exports include install and typing entry points
- [ ] Example app exercises the packaged component
- [ ] Core behavior is covered by tests
