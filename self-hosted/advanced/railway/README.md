# Self-hosting Convex with [Railway.com](https://railway.com/)

Instructions for railway self-hosting is community maintained. For help, please
join the [community discord](https://convex.dev/community). Thanks to
[orenaksakal](https://github.com/orenaksakal) for the work in putting the
instructions together.

## Deploying to Railway.com

You can view the ready to deploy template here

- [Convex + SQLite](https://railway.com/deploy/convex-1)
- [Convex + Postgres](https://railway.com/deploy/convex-and-postgres-official)
- [Convex + Mysql](https://railway.com/deploy/convex-mysql)

or use one click deploy buttons below:

Convex + SQLite

[![Deploy on Railway.com](https://railway.com/button.svg)](https://railway.com/deploy/convex-1)

Convex + Postgres

[![Deploy on Railway.com](https://railway.com/button.svg)](https://railway.com/deploy/convex-and-postgres-official)

Convex + Mysql

[![Deploy on Railway.com](https://railway.com/button.svg)](https://railway.com/deploy/convex-mysql)

## Setup

The template comes with pre-configured env-variables and you need to follow some
setup steps to make it fully work.

1. Deploying the template
2. Generating admin key with railway ssh

Steps:

1. Deploying the template

   Just deploy the template and enjoy. 🚀

   ℹ️ (Optional): If you would like to separete api and http domains follow
   steps below:
   - Select `convex-backend` service
   - Select Settings tab and scroll to `Public Networking` section
   - Hover on the domain and click on edit or delete buttons
   - Click on `Generate Domain` for auto generated one or `Custom Domain` if you
     want to setup custom domain
   - Make sure to select port `3210` and add your domain for convex (api) url
     and select port `3211` for http (action) routes
   - Re-deploy both `convex-dashboard` and `convex-backend` services

2. Generating admin key with railway ssh

   Follow [these](https://blog.railway.com/p/ssh#how-to-ssh-on-railway)
   instructions to setup railway SSH on your machine
   - Link your convex deployment project
   - Run `railway ssh` and select `convex-backend` when prompted
   - Run `ls` and then `./generate_admin_key.sh`
   - Copy the whole admin key logged on the screen
   - This is your admin key keep it secret

### HTTP Actions

Note that HTTP actions run on your railway app url under the `/http` path. For
example:

- If your railway app is deployed at `https://self-hosted-backend.railway.app`
- And you have an HTTP action routed to `/sendEmail`
- You would call it at `https://self-hosted-backend.railway.app/http/sendEmail`

### Database

If you deployed SQLite template, your data is stored in local SQLite file and
your files are stored in the Railway volume. You can see them in the `data`
folder if you run:

```
railway ssh
ls
```

To store your data in a SQL database of your choice, see
[these instructions](https://github.com/convex/convex-backend/tree/main/self-hosted/README.md#running-the-database-on-postgres--or-mysql).

## Accessing the deployed dashboard

The dashboard allows you to see logs, read/write data, run functions, and more.
You can run the dashboard locally with Docker, or deploy it to Railway.

- Head over to your railway app
- Select `convex-dashboard`
- Visit its public url
- Paste the admin key when prompted
- Enjoy

### Running the dashboard locally

```sh
docker run -e 'NEXT_PUBLIC_DEPLOYMENT_URL=<backend-url>' -p '6791:6791' 'ghcr.io/convex/convex-dashboard:latest'
```

## Deploying your frontend app

See
[these instructions](https://github.com/convex/convex-backend/tree/main/self-hosted/README.md#deploying-your-frontend-app).

## Troubleshooting

- **Performance issues**: The default railway configuration allocates the
  minimum possible resources to get up and running. If your app has high load,
  you may see ratelimiting from railway and poor performance. We recommend
  increasing your memory and CPU.
- **Running out of disk space**: The hobby railway configuration allocates 5GB
  to the `convex_data` volume where your SQLite database and storage lives. If
  you run out of space, you can increase the volume to 50GB by upgrading plan.
- If you need more help feel free to join our discord
  [community discord](https://convex.dev/community)
