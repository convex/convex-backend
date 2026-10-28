"use client";

import { Preloaded, usePreloadedQuery, useQuery } from "convex/react";
import { api } from "../../convex/_generated/api";

export default function Home({
  preloaded,
}: {
  preloaded: Preloaded<typeof api.tasks.user>;
}) {
  const tasks = useQuery(api.tasks.get);
  const user = usePreloadedQuery(preloaded);
  return (
    <main className="flex min-h-screen flex-col items-center justify-between p-24">
      {tasks?.map(({ _id, text }) => (
        <div key={_id}>{text}</div>
      ))}
      <code>
        <pre>{JSON.stringify(user, null, 2)}</pre>
      </code>
    </main>
  );
}
