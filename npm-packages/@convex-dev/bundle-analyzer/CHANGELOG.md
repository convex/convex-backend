# Changelog

## Unreleased

- The package is no longer marked as alpha.
- For devDependencies imported by application code, `dev-dependency-bundled`
  insights list the importing files in `items[].importedBy`.

## 0.1.0

- Initial release. Run `npx @convex-dev/bundle-analyzer summary` from the root
  of a Convex project to see an overview of its bundle. Other commands:
  `insights`, `components`, `packages`, `files`, `modules`, and `explore`. Every
  command except `explore` takes `--json`.
