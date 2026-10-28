import { SignInButton, UserButton } from "@clerk/react";
import { useQuery } from "convex/react";
import { api } from "../convex/_generated/api";
import { useStoreUserEffect } from "./useStoreUserEffect.js";

function App() {
  const { isLoading, isAuthenticated } = useStoreUserEffect();
  return (
    <main>
      {isLoading ? (
        <>Loading...</>
      ) : !isAuthenticated ? (
        <SignInButton />
      ) : (
        <>
          <UserButton />
          <Content />
        </>
      )}
    </main>
  );
}

function Content() {
  const messages = useQuery(api.messages.getForCurrentUser);
  return <div>Authenticated content: {messages?.length}</div>;
}

export default App;
