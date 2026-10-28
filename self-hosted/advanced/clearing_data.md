# Clearing or resetting data

There is no single destructive "reset database" command, by design. Instead, you
clear data by replacing tables with an empty snapshot import. This lets you wipe
data without deleting your Docker volume or re-provisioning the deployment, and
it leaves your schema and functions in place.

> Back up first: run `npx convex export --path backup.zip` before clearing.
> `--replace -y` is irreversible.

Before running an all-tables clear, make sure `CONVEX_SELF_HOSTED_URL` points at
the deployment you intend to wipe. The same one-liner run against a production
deployment destroys all of its data.

## Clear a single table

```
npx convex import --table $tableName --replace --format jsonLines /dev/null -y
```

## Clear all tables in the app

```
for tableName in `npx convex data`; do npx convex import --table $tableName --replace -y --format jsonLines /dev/null; done
```

## Clear all tables in a component

```
for tableName in `npx convex data --component $component`; do npx convex import --component $component --table $tableName --replace -y --format jsonLines /dev/null; done
```

These commands use `/dev/null` as an empty import file, which works on Linux and
macOS. On Windows (PowerShell), `/dev/null` does not exist, so use an empty file
you create instead.

Commands verified with `convex@latest` as of 2026-06. Run
`npx convex import --help` for current flag behavior.
