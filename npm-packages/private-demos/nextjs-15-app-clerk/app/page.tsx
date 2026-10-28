"use client";

import { useQuery } from "convex/react";
import { api } from "../convex/_generated/api";
import { ConvexClientProvider } from "./ConvexClientProvider";
import { SignInButton, SignOutButton } from "@clerk/nextjs";

export default function Home() {
  return (
    <ConvexClientProvider>
      <HomeInner />
    </ConvexClientProvider>
  );
}
const HomeInner = () => {
  const tasks = useQuery(api.tasks.get);
  const user = useQuery(api.tasks.user);
  return (
    <main className="flex min-h-screen flex-col items-center justify-between p-24">
      {tasks?.map(({ _id, text }) => (
        <div key={_id}>{text}</div>
      ))}
      <code>
        <pre>{JSON.stringify(user, null, 2)}</pre>
      </code>

      <SignInButton />
      <SignOutButton />
    </main>
  );
};
