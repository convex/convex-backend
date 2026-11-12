## Running the dashboard locally

From the `npm-packages/dashboard-self-hosted` directory, run:

```sh
just install-js
just turbo run build --filter=dashboard-self-hosted^...
npm run build
NEXT_PUBLIC_DEPLOYMENT_URL="<your-backend-url>" npm run start
```

## Dashboard optional configuration

- The dashboard uses the **monaco-editor** npm package for all the editor-like
  elements. By default, monaco loads it's core from a CDN. You could configure
  it to load internally by setting the `NEXT_PUBLIC_LOAD_MONACO_INTERNALLY`
  environment variable to `true`
- `NEXT_PUBLIC_ADMIN_KEY` pre-fills the admin key on the login screen.
- To skip the login screen entirely, set `NEXT_PUBLIC_SKIP_LOGIN` to `true`
  together with `NEXT_PUBLIC_DEPLOYMENT_URL` and `NEXT_PUBLIC_ADMIN_KEY`. The
  dashboard then logs in with these credentials automatically. Anyone who can
  open the dashboard gets full admin access to the deployment, so only use this
  when access to the dashboard is protected by other means, such as an
  authenticating proxy.
