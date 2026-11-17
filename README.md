<p align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://static.convex.dev/logo/convex-logo-light.svg" width="600">
  <source media="(prefers-color-scheme: light)" srcset="https://static.convex.dev/logo/convex-logo.svg" width="600">
  <img alt="Convex logo" src="https://static.convex.dev/logo/convex-logo.svg" width="600">
</picture>
</p>

[Convex](https://convex.dev) is the open-source reactive database designed to
make life easy for web app developers, whether human or LLM. Fetch data and
perform business logic with strong consistency by writing pure TypeScript.

Convex provides a database, a place to write your server functions, and client
libraries. It makes it easy to build and scale dynamic live-updating apps.
[Read the docs to learn more](https://docs.convex.dev/understanding/).

Development of the Convex backend is led by the Convex team. We
[welcome small bug fixes](./CONTRIBUTING.md) and
[love receiving feedback](https://discord.gg/convex). We keep this repository
synced with any internal development work within a handful of days. Convex is a
well tested piece of software, with several well designed test frameworks
including randomized testing. Those tests are not provided as part of the open
source offering.

## Getting Started

Visit our [documentation](https://docs.convex.dev/) to learn more about Convex
and follow our getting started guides.

The easiest way to build with Convex is through our
[cloud platform](https://www.convex.dev/plans), which includes a generous free
tier and lets you focus on building your application without worrying about
infrastructure. Many small applications and side-projects can operate entirely
on the free tier with zero cost and zero maintenance.

## Self Hosting

The self-hosted product includes most features of the cloud product, including
the dashboard and CLI. Self-hosted Convex works well with a variety of tools
including Neon, Fly.io, Vercel, Netlify, RDS, Sqlite, Postgres, and more.

You can either use Docker (recommended) or a prebuilt binary to self host
Convex. Check out our [self-hosting guide](./self-hosted/README.md) for detailed
instructions. Community support for self-hosting is available in the
`#self-hosted` channel on [Discord](https://discord.gg/convex).

## Community & Support

- Join our [Discord community](https://discord.gg/convex) for help and
  discussions.
- Report issues when building and using the open source Convex backend through
  [GitHub Issues](https://github.com/convex/convex-backend/issues)
- By submitting pull requests, you confirm that Convex can use, modify, copy,
  and redistribute the contribution, under the terms of its choice.

## Building from source

See [BUILD.md](./BUILD.md).

## Disclaimers

- If you choose to self-host, we recommend following the self-hosting guide. If
  you are instead building from source, make sure to change your instance secret
  and admin key from the defaults in the repo.
- Convex is battle tested most thoroughly on Linux and Mac. On Windows, it has
  less experience. If you run into issues, please message us on
  [Discord](https://convex.dev/community) in the `#self-hosted` channel.
- Convex self-hosted builds contain a beacon to help Convex improve the product.
  The information is minimal and anonymous and helpful to Convex, but if you
  really want to disable it, you can set the `--disable-beacon` flag on the
  backend binary. The beacon's messages print in the log and only include
  - A random identifier for your deployment (not used elsewhere)
  - Migration version of your database
  - Git rev of the backend
  - Uptime of the backend

## Repository layout

- `crates/` contains Rust code
  - Main binary
    - `local_backend/` is an application server on top of the `Runtime`. This is
      the serving edge for the Convex cloud.

- `npm-packages/` contains both our public and internal TypeScript packages.
  - Internal packages
    - `udf-runtime/` sets up the user-defined functions JS environment for
      queries and mutations
    - `system-udfs/` contains functions used by the Convex system e.g. the CLI
