<p align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://static.convex.dev/logo/convex-logo-light.svg" width="600">
  <source media="(prefers-color-scheme: light)" srcset="https://static.convex.dev/logo/convex-logo.svg" width="600">
  <img alt="Convex logo" src="https://static.convex.dev/logo/convex-logo.svg" width="600">
</picture>
</p>

If you're new to Convex we recommend starting with the
[onboarding tutorial](https://docs.convex.dev/tutorial/) to familiarize yourself
with the Convex development experience.

If you're in this README, you're interested in self-hosting
[Convex](https://www.convex.dev) on your own infrastructure or a managed hosting
provider. Support is available on the
[Convex Discord](https://discord.gg/convex) in the `#self-hosted` channel.

If you don't specifically want to self-host, head over to
[the Convex docs](https://docs.convex.dev/) to use the cloud-hosted product.
Cloud-hosted Convex includes a generous free tier and provides a seamless,
reliable, cost-effective platform that allows you to focus on building your
application without worrying about infrastructure.

Self-hosting Convex requires deploying three services:

1. The Convex backend
2. The Convex dashboard
3. Your frontend app, which you can either host yourself or on a managed service
   like Netlify or Vercel.

# Self-hosting Convex

By default the Convex backend will store all state in a local SQLite database.
We recommend starting with this basic configuration and then moving the
container to a hosting provider or pointing the backend to a separate SQL
database for a production-ready configuration as needed.

## Docker configuration

First download the
[`docker-compose.yml` file](https://github.com/convex/convex-backend/tree/main/self-hosted/docker/docker-compose.yml).
Then, to start the backend and dashboard:

```sh
docker compose up
```

Once the backend is running you can use it to generate admin keys for the
dashboard/CLI:

```sh
docker compose exec backend ./generate_admin_key.sh
```

Visit the dashboard at `http://localhost:6791`. The backend listens on
`http://127.0.0.1:3210`. The backend's http actions are available at
`http://127.0.0.1:3211`.

In your Convex project, add your url and admin key to a `.env.local` file (which
should not be committed to source control):

```sh
CONVEX_SELF_HOSTED_URL='http://127.0.0.1:3210'
CONVEX_SELF_HOSTED_ADMIN_KEY='<your admin key>'
```

Now you can run commands in your Convex project, to push code, run queries,
import data, etc. To use these commands, you'll need the latest version of
Convex.

```sh
npm install convex@latest
```

Now you can push code, run queries, import data, etc.

```sh
npx convex dev
npx convex --help  # see all available commands
```

By default, the backend will store its data in a volume managed by Docker. Note
that you'll need to set up persistent storage on whatever cloud hosting platform
you choose to run the Docker container on (e.g. AWS EBS). By default the
database is stored locally in SQLite but you may also point it to a SQL database
either locally or on a cloud service of your choice following
[these instructions](./advanced/postgres_or_mysql.md). You can also configure
the backend to use S3 storage for exports, snapshots, modules, files, and search
indexes following [these instructions](./advanced/s3_storage.md).

You should now be able to use the self-hosted backend. Read on for alternative
hosting options for production workloads.

## Convex Auth

If you're using Convex Auth, follow the
[manual instructions](https://labs.convex.dev/auth/setup/manual) to set up. The
CLI does not support self-hosted deployments yet.

# Deploying your frontend app

The Convex backend runs all database and compute functions but it doesn't host
your actual web app. If you're hosting your website on a provider like Netlify
or Vercel using our
[production hosting instructions](https://docs.convex.dev/production/hosting/)
be sure to swap out the environment variables in those instructions for the
`SELF_HOSTED` equivalents.

e.g., instead of setting `CONVEX_DEPLOY_KEY`, you'll need to set
`CONVEX_SELF_HOSTED_URL` to the url where your Convex backend is hosted and
`CONVEX_SELF_HOSTED_ADMIN_KEY` to the admin key you generated with the
`generate_admin_key.sh` script.

## Advanced Configuration

- [Running the binary directly](./advanced/running_binary_directly.md)
- [Hosting on fly.io](./advanced/fly/README.md)
- [Hosting on Railway.com](./advanced/railway/README.md)
- [Hosting on your own servers](./advanced/hosting_on_own_infra.md)
- [Running the database on Postgres or MySQL](./advanced/postgres_or_mysql.md)
- [Storing files in S3 instead of local filesystem](./advanced/s3_storage.md)
- [Clearing or resetting data](./advanced/clearing_data.md)
- [Running the dashboard locally](./advanced/dashboard.md)
- [Disabling logging features](./advanced/disabling_logging.md)
- [Upgrading Convex self-hosted version](./advanced/upgrading.md)
- [Benchmarking](./advanced/benchmarking.md)
- [Advanced tuning with knobs](./advanced/knobs.md)

# Limitations

Self-hosted Convex supports all the free-tier features of the cloud-hosted
product. The cloud-hosted product is optimized for scale.

# Questions and contributions

- Join our [Discord community](https://discord.gg/convex) for help and
  discussions. The `#self-hosted` channel is the best place to go for questions
  about self-hosting.

- Report issues when building and using the open source Convex backend through
  [GitHub Issues](https://github.com/convex/convex-backend/issues)

- We
  [welcome bug fixes](https://github.com/convex/convex-backend/blob/main/crates/convex/CONTRIBUTING.md)
  and love receiving feedback. We keep this repository synced with any internal
  development work within a handful of days.
