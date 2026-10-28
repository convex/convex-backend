## Safe Environment Variable Playground

You can experiment with how defining safe environment variables work in this
demo.

Modify the `convex/convex.config.ts` file to add or remove declared env vars and
`convex/functions.ts` to make use of them.

The app hooks up two components so you can see both env var shapes:
`fakecomponent` declares a required env var (so `env` must be passed when
installing it) and `optionalcomponent` declares only an optional one (so `env`
can be omitted). Try changing either `convex.config.ts` to experiment.

Spin up a backend using `just run-local-backend` and try your changes with
`just convex dev`.

You can also try adding/deleting/changing environment variables on the dashboard
after starting it with `just run-dashboard-local-backend`.
