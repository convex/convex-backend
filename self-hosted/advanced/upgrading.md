# Upgrading self-hosted Convex

In order to safely migrate to a new version of self-hosted, there are two
options.

## Option 1: Upgrade in-place

If you want to avoid downtime, you can upgrade in-place. It is highly
recommended that you run an `npx convex export` before you upgrade so that you
can restore in case something goes wrong.

Look for loglines like this - and follow those instructions to complete the
in-place upgrade. Each migration will let you know which logline to wait for to
determine that the in-place upgrade is complete.

```
Executing Migration 114/115. MigrationComplete(115)
```

This will migrate your existing database in-place toward a new one. There may be
rare cases in which this does not work smoothly, in which case you can try
option 2.

## Option 2: Export/Import your database

This allows you to start over your database from scratch, and import your data
directly, instead of trying to migrate in-place.

1. Take down external traffic to your backend.
2. Export your database with `npx convex export`.
3. Save your environment variables with
   `npx convex env list --include-values > .env.convex` (or via dashboard).
4. Upgrade the backend docker image.
5. Import from your backup with `npx convex import --replace-all`.
6. Bring back your environment variables with
   `npx convex env set --force --from-file .env.convex` (or via dashboard)
7. Bring back external traffic to your backend.

Given that exports/imports can be expensive if you have a lot of data, this can
incur downtime. You can get a sense of how much downtime by running a test
export while your self-hosted instance is up. For smaller instances, this may be
quick and easy.

However to safely avoid losing data, it's important that the final export is
done after load is stopped from your instance, since exports are taken at a
snapshot in time.
