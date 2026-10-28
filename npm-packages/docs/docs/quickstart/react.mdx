---
title: React Quickstart
sidebar_label: React
description: "Add Convex to a React project"
hide_table_of_contents: true
sidebar_position: 100
---

import sampleData from "!!raw-loader!@site/../private-demos/quickstarts/react-vite/sampleData.jsonl";
import tasks from "!!raw-loader!@site/../private-demos/quickstarts/react-vite-ts/convex/tasks.ts";
import main from "!!raw-loader!@site/../private-demos/quickstarts/react-vite-ts/src/main.tsx";
import App from "!!raw-loader!@site/../private-demos/quickstarts/react-vite-ts/src/App.tsx";

<iframe
  width="560"
  height="315"
  src="https://www.youtube.com/embed/4MgsvjMb59Q"
  title="YouTube video player"
  frameborder="0"
  allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
  referrerpolicy="strict-origin-when-cross-origin"
  allowfullscreen
></iframe>

To get setup quickly with Convex and React run

<p>
  <b>
    <CodeWithCopyButton text="npm create convex@latest" />
  </b>
</p>

or follow the guide below.

---

Learn how to query data from Convex in a React app using Vite.

<StepByStep>
  <Step title="Create a React app">
    Create a React app using the `create vite` command.

    ```sh
    npm create vite@latest my-app -- --template react-ts
    ```

  </Step>
  <Step title="Install the Convex client and server library">
    To get started, install the `convex`
    package which provides a convenient interface for working
    with Convex from a React app.

    Navigate to your app directory and install `convex`.


    ```sh
    cd my-app && npm install convex
    ```

  </Step>
  <Step title="Set up a Convex dev deployment">
    Next, run `npx convex dev`. This
    will prompt you to log in with GitHub,
    create a project, and save your production and deployment URLs.

    It will also create a `convex/` folder for you
    to write your backend API functions in. The `dev` command
    will then continue running to sync your functions
    with your dev deployment in the cloud.


    ```sh
    npx convex dev
    ```

  </Step>

  <Step title="Create sample data for your database">
    In a new terminal window, create a `sampleData.jsonl`
    file with some sample data.

    <Snippet
      source={sampleData}
      title="sampleData.jsonl"
    />

  </Step>

  <Step title="Add the sample data to your database">
    Now that your project is ready, add a `tasks` table
    with the sample data into your Convex database with
    the `import` command.

    ```sh
    npx convex import --table tasks sampleData.jsonl
    ```

  </Step>

  <Step title="(optional) Define a schema">
    Add a new file `schema.ts` in the `convex/` folder
    with a description of your data.

    This will declare the types of your data for optional
    typechecking with TypeScript, and it will be also
    enforced at runtime.

    ```ts noDialect title="convex/schema.ts"
    import { defineSchema, defineTable } from "convex/server";
    import { v } from "convex/values";

    export default defineSchema({
      tasks: defineTable({
        text: v.string(),
        isCompleted: v.boolean(),
      }),
    });
    ```

  </Step>

  <Step title="Expose a database query">
    Add a new file `tasks.ts` in the `convex/` folder
    with a query function that loads the data.

    Exporting a query function from this file
    declares an API function named after the file
    and the export name, `api.tasks.get`.

    <Snippet
      source={tasks}
      title="convex/tasks.ts"
    />

  </Step>

  <Step title="Connect the app to your backend">
    In `src/main.tsx`, create a `ConvexReactClient` and pass it to a `ConvexProvider`
    wrapping your app.

    <Snippet
      source={main}
      title="src/main.tsx"
      highlightPatterns={[ "Convex", ]}
    />

  </Step>

  <Step title="Display the data in your app">
    In `src/App.tsx`, use the `useQuery` hook to fetch from your `api.tasks.get`
    API function and display the data.

    <Snippet
      source={App}
      title="src/App.tsx"
      highlightPatterns={[ "useQuery", "api", "tasks", "text" ]}
    />

  </Step>

  <Step title="Start the app">
    Start the app, open [http://localhost:5173/](http://localhost:5173/) in a browser,
    and see the list of tasks.

    ```sh
    npm run dev
    ```

  </Step>

</StepByStep>

See the complete [React documentation](/client/react/overview.mdx).
