# Convex Tour Chat

This is a sample app used in the convex tutorial to show off the fundamentals of
the Convex platform.

[Go check out the tutorial](https://convex.dev/start)

Jump into any particular step of the tutorial with a branch:

1.  Simple chat app with message display order bug (branch = main, 0-start)
1.  Simple chat app with correct message display (branch = 1-smileys)
1.  Enhanced chat app a "likes" feature added (branch = 2-likes)
1.  Enhanced chat app with an AI integration (branch = 3-ai)

# To run

    $ npm i
    $ npx convex init

This app declares `TOGETHER_API_KEY` in `convex/convex.config.ts`, so the
deployment won't accept code until it is set. Get a key at
[together.ai](https://together.ai/), then run this and paste it at the prompt,
which keeps it hidden and out of your shell history:

    $ npx convex env set TOGETHER_API_KEY

You can also set it from your [Convex dashboard](https://dashboard.convex.dev/).
Then start dev:

    $ npm run dev
