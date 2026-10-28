## Backend hosting on your own infrastructure

It's possible to run Convex on your own servers, with your own routing.

Download the
[`docker-compose.yml` file](https://github.com/convex/convex-backend/tree/main/self-hosted/docker/docker-compose.yml)
onto the server you want to run Convex on.

```sh
curl -O https://raw.githubusercontent.com/convex/convex-backend/main/self-hosted/docker/docker-compose.yml
```

Your Convex backend will be running on this server at port 3210, with HTTP
actions exposed at port 3211, and the dashboard running on port 6791.

Set up routing to forward requests from your domain to these ports. For example:

- `https://api.my-domain.com` forwards to `http://localhost:3210`
- `https://my-domain.com` forwards to `http://localhost:3211`
- `https://dashboard.my-domain.com` forwards to `http://localhost:6791`

In a `.env` file beside the `docker-compose.yml` file, set the following
environment variables:

```sh
# URL of the Convex API as accessed by the client/frontend.
CONVEX_CLOUD_ORIGIN='https://api.my-domain.com'
# URL of Convex HTTP actions as accessed by the client/frontend.
CONVEX_SITE_ORIGIN='https://my-domain.com'
# URL of the Convex API as accessed by the dashboard (browser).
NEXT_PUBLIC_DEPLOYMENT_URL='https://api.my-domain.com'
```

On the server, start the backend with:

```sh
docker compose up
```

Get an admin key with:

```sh
docker compose exec backend ./generate_admin_key.sh
```

Go to the dashboard at `https://dashboard.my-domain.com` and use the admin key
to authenticate.

In your Convex project (on your local machine, probably not on the hosting
server), add the url and admin key to a `.env.local` file (which should not be
committed to source control):

```sh
CONVEX_SELF_HOSTED_URL='https://api.my-domain.com'
CONVEX_SELF_HOSTED_ADMIN_KEY='<your admin key>'
```

Now you can run commands in your Convex project, to push code, run queries,
import data, etc.

```sh
npx convex dev
```
