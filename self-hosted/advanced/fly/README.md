# Self-hosting Convex with [Fly.io](https://fly.io/)

We include `fly.toml` files to make it easy to deploy your backend to
[Fly.io](https://fly.io/).

## Setup

Copy the
[`fly` files](https://github.com/convex/convex-backend/tree/main/self-hosted/advanced/fly)
to your local machine. You don't need to copy it into your project directory,
but you can. `degit` is a tool for copying files from git repositories.

```sh
npx degit convex/convex-backend/self-hosted/advanced/fly fly
cd fly
```

Install the `fly` CLI by following
[these instructions](https://fly.io/docs/flyctl/install/)

## Deploying the backend to Fly.io

The backend "deploy" can mean two things:

1. Deploying the Convex backend docker image to Fly.io.
2. Deploying your app's Convex functions to the fly machine running Convex.

Steps:

1. Deploy the backend to Fly.io.

   ```sh
   cd backend
   fly launch
   ```

   When prompted, hit 'y' to copy the configuration to your new app. Edit the
   location to be in the same region as your database (if you're using
   [Postgres or MySQL](https://github.com/convex/convex-backend/tree/main/self-hosted/advanced/postgres_or_mysql.md)).
   Now you have a fly app that is running the Convex backend, where you can
   deploy your app's Convex functions.

   Note the URL of the app that gets printed out, which will be of the form
   `https://<app-name>.fly.dev`. We'll call this the `fly-backend-url`.

2. Set the environment variables `CONVEX_CLOUD_ORIGIN` and `CONVEX_SITE_ORIGIN`
   for your backend.

   These environment variables are used by the backend so it knows where it is
   hosted. Inside your Convex backend functions, you can access the backend's
   URL with `process.env.CONVEX_CLOUD_URL` for the Convex client API and
   `process.env.CONVEX_SITE_URL` for the HTTP API.

   **Using the fly.toml file:**

   To save them in the fly.toml file, set them in the `[env]` section.

   ```toml
   [env]
   TMPDIR = '/convex/data/tmp'
   CONVEX_CLOUD_ORIGIN = '<fly-backend-url>'
   CONVEX_SITE_ORIGIN = '<fly-backend-url>/http'
   ```

   Then re-deploy to pick up the changes.

   ```sh
   fly deploy
   ```

   **Storing them as secrets:**

   If you'd rather not save the env variables in the fly.toml file, you can set
   them as secrets with `fly secrets set`. This can be useful if multiple
   developers are working on the same repo where the fly.toml file is already
   checked in, and they each have their own fly.io-hosted backend.

   ```sh
   fly secrets set CONVEX_CLOUD_ORIGIN="<fly-backend-url>" CONVEX_SITE_ORIGIN="<fly-backend-url>/http"
   ```

   Now your backend knows its base URL so it can generate URLs that point back
   to itself. This is especially useful for libraries registering webhooks and
   [Convex Auth](https://labs.convex.dev/auth) for generating auth callbacks.

3. Check that the backend is running. Visit the `<fly-backend-url>` and you
   should see a message saying your backend is running! Check the logs with
   `fly logs` if it's not working.

4. Generate an admin key.

   ```sh
   fly ssh console --command "./generate_admin_key.sh"
   ```

   This admin key will be used to authorize the CLI and access the dashboard.

5. Inside your app that uses Convex, create a `.env.local` file with the
   following variables:

   ```sh
   CONVEX_SELF_HOSTED_URL='<fly-backend-url>'
   CONVEX_SELF_HOSTED_ADMIN_KEY='<your-admin-key>'
   ```

6. To deploy your Convex functions to the backend, you'll use the `convex` CLI.

   If you don't already have Convex installed for your app, install it.

   ```sh
   cd <your-frontend-app-directory>
   npm install convex@latest
   ```

   To continuously deploy code for development:

   ```sh
   npx convex dev
   ```

   This will continuously deploy your Convex functions as you edit them. It will
   also set environment variables in `.env.local` for your frontend, like
   `VITE_CONVEX_URL`.

   To deploy code once:

   ```sh
   npx convex deploy --env-file <path to env file>
   ```

   To deploy code to a different backend, you can pass the `--env-file` flag or
   set the self-hosted environment variables before calling `npx convex deploy`.

   **Note:** It's up to you whether a backend is for development or production.
   There is no distinction within the instance. If you only have one backend,
   you can run `npx convex dev` or `npx convex deploy` depending on whether you
   want it to live-update or not.

   An extension of this is that you can have many backends for staging or
   previews. The difference will be in the environment variables.

### HTTP Actions

Note that HTTP actions run on your fly app url under the `/http` path. For
example:

- If your fly app is deployed at `https://self-hosted-backend.fly.dev`
- And you have an HTTP action routed to `/sendEmail`
- You would call it at `https://self-hosted-backend.fly.dev/http/sendEmail`

### Database

At this point, your data is stored in SQLite and your files are stored in the
filesystem in your Fly volume. You can see them in the `data` folder if you run:

```
fly ssh console
ls
```

To store your data in a SQL database of your choice, see
[these instructions](https://github.com/convex/convex-backend/tree/main/self-hosted/advanced/postgres_or_mysql.md).

## Deploying the dashboard

The dashboard allows you to see logs, read/write data, run functions, and more.
You can run the dashboard locally with Docker, or deploy it to Fly.io.

### Running the dashboard locally

```sh
docker run -e 'NEXT_PUBLIC_DEPLOYMENT_URL=<fly-backend-url>' -p '6791:6791' 'ghcr.io/convex/convex-dashboard:latest'
```

### Fly.io deployment

1. Go into the dashboard directory where you copied the self-hosted files.

   ```sh
   cd dashboard
   ```

2. Deploy the dashboard to Fly.io with the url to your fly-hosted backend.

   **Saving the url in the fly.toml file:**

   ```sh
   fly launch -e NEXT_PUBLIC_DEPLOYMENT_URL="<fly-backend-url>"
   ```

   **Saving the url as a secret:**

   If you'd rather not have the url in the fly.toml file, you can set it as a
   secret. This can be useful if multiple developers are working on the same
   repo where the fly.toml file is already checked in, and they each have their
   own fly.io-hosted dashboard.

   ```sh
   fly launch
   fly secrets set NEXT_PUBLIC_DEPLOYMENT_URL="<fly-backend-url>"
   ```

   You should now be able to visit the dashboard at the url output by fly.

3. Visit the dashboard and enter the admin key. To log in, it will need the
   admin key you generated earlier. We recommend saving it in your password
   manager for ease, but you can always regenerate it with
   `fly ssh console --command "./generate_admin_key.sh"`.

   You should see your tables, functions, logs, etc.

## Deploying your frontend app

See
[these instructions](https://github.com/convex/convex-backend/tree/main/self-hosted/README.md#deploying-your-frontend-app).

## Troubleshooting

- **Performance issues**: The default fly configuration allocates the minimum
  possible resources to get up and running. If your app has high load, you may
  see ratelimiting from Fly and poor performance. We recommend increasing your
  memory and CPU.
- **Running out of disk space**: The default fly configuration allocates 1GB to
  the `convex_data` volume where your SQLite database and storage lives. If you
  run out of space, you can increase the volume size with `fly volume extend`.
