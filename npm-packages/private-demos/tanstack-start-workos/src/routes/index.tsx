import { Link, createFileRoute } from '@tanstack/react-router';
import { Authenticated, Unauthenticated, useMutation } from 'convex/react';
import { useAuth } from '@workos/authkit-tanstack-react-start/client';
import { getAuth } from '@workos/authkit-tanstack-react-start';
import { convexQuery } from '@convex-dev/react-query';
import { useSuspenseQuery } from '@tanstack/react-query';
import { api } from '../../convex/_generated/api';
import type { User } from '@workos/authkit-tanstack-react-start';

export const Route = createFileRoute('/')({
  component: Home,
  loader: async () => {
    const { user } = await getAuth();
    return { user };
  },
});

function Home() {
  const { user } = Route.useLoaderData();
  return <HomeContent user={user} />;
}

function HomeContent({ user }: { user: User | null }) {
  return (
    <>
      <header className="sticky top-0 z-10 bg-background p-4 border-b-2 border-slate-200 dark:border-slate-800 flex flex-row justify-between items-center">
        Convex + TanStack Start + WorkOS
        {user && <UserMenu user={user} />}
      </header>
      <main className="p-8 flex flex-col gap-8">
        <h1 className="text-4xl font-bold text-center">Convex + TanStack Start + WorkOS</h1>
        <Authenticated>
          <Content />
        </Authenticated>
        <Unauthenticated>
          <SignInForm />
        </Unauthenticated>
      </main>
    </>
  );
}

function SignInForm() {
  return (
    <div className="flex flex-col gap-8 w-96 mx-auto">
      <p>Log in to see the numbers</p>
      <a href="/sign-in">
        <button className="bg-foreground text-background px-4 py-2 rounded-md">Sign in</button>
      </a>
      <a href="/sign-up">
        <button className="bg-foreground text-background px-4 py-2 rounded-md">Sign up</button>
      </a>
    </div>
  );
}

function Content() {
  const {
    data: { viewer, numbers },
  } = useSuspenseQuery(
    convexQuery(api.myFunctions.listNumbers, {
      count: 10,
    }),
  );
  const addNumber = useMutation(api.myFunctions.addNumber);

  return (
    <div className="flex flex-col gap-8 max-w-lg mx-auto">
      <p>Welcome {viewer}!</p>
      <p>
        Click the button below and open this page in another window - this data is persisted in the Convex cloud
        database!
      </p>
      <p>
        <button
          className="bg-foreground text-background text-sm px-4 py-2 rounded-md"
          onClick={() => {
            void addNumber({ value: Math.floor(Math.random() * 10) });
          }}
        >
          Add a random number
        </button>
      </p>
      <p>Numbers: {numbers.length === 0 ? 'Click the button!' : numbers.join(', ')}</p>
      <p>
        Edit{' '}
        <code className="text-sm font-bold font-mono bg-slate-200 dark:bg-slate-800 px-1 py-0.5 rounded-md">
          convex/myFunctions.ts
        </code>{' '}
        to change your backend
      </p>
      <p>
        Edit{' '}
        <code className="text-sm font-bold font-mono bg-slate-200 dark:bg-slate-800 px-1 py-0.5 rounded-md">
          src/routes/index.tsx
        </code>{' '}
        to change your frontend
      </p>
      <p>
        See{' '}
        <Link to="/authenticated" className="underline hover:no-underline">
          /authenticated
        </Link>{' '}
        for an example of a page only available to authenticated users.
      </p>
      <div className="flex flex-col">
        <p className="text-lg font-bold">Useful resources:</p>
        <div className="flex gap-2">
          <div className="flex flex-col gap-2 w-1/2">
            <ResourceCard
              title="Convex docs"
              description="Read comprehensive documentation for all Convex features."
              href="https://docs.convex.dev/home"
            />
            <ResourceCard
              title="Stack articles"
              description="Learn about best practices, use cases, and more from a growing collection of articles, videos, and walkthroughs."
              href="https://stack.convex.dev"
            />
          </div>
          <div className="flex flex-col gap-2 w-1/2">
            <ResourceCard
              title="Templates"
              description="Browse our collection of templates to get started quickly."
              href="https://www.convex.dev/templates"
            />
            <ResourceCard
              title="Discord"
              description="Join our developer community to ask questions, trade tips & tricks, and show off your projects."
              href="https://www.convex.dev/community"
            />
          </div>
        </div>
      </div>
    </div>
  );
}

function ResourceCard({ title, description, href }: { title: string; description: string; href: string }) {
  return (
    <div className="flex flex-col gap-2 bg-slate-200 dark:bg-slate-800 p-4 rounded-md h-28 overflow-auto">
      <a href={href} className="text-sm underline hover:no-underline">
        {title}
      </a>
      <p className="text-xs">{description}</p>
    </div>
  );
}

function UserMenu({ user }: { user: User }) {
  const { signOut } = useAuth();

  return (
    <div className="flex items-center gap-2">
      <span className="text-sm">{user.email}</span>
      <button onClick={() => signOut()} className="bg-red-500 text-white px-3 py-1 rounded-md text-sm hover:bg-red-600">
        Sign out
      </button>
    </div>
  );
}
