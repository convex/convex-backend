# dashboard-self-hosted

This is the dashboard for the self-hosted version of Convex. You may run this
code locally to test changes.

## Configuration

To use the scripts set up in this repo you'll need to install
[`Just`](https://github.com/casey/just)

- Just is used to execute scripts set up in the `Justfile`.
- To install it see
  [Packages](https://github.com/casey/just?tab=readme-ov-file#packages), for
  example `cargo install just` or `brew install just`

Next you'll need the URL to your Convex deployment. This URL can point to a
deployment hosted locally, remotely, or on Convex Cloud (URL found on the
deployment settings page: https://dashboard.convex.dev/deployment/settings)

One time setup:

```
# Install dependencies
just install-js

# Build the project's dependencies
just turbo run build --filter=dashboard-self-hosted^...
```

Run the dashboard:

```
just run-dashboard "YOUR_DEPLOYMENT_URL"
```
