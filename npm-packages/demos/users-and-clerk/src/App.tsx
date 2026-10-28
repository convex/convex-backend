import { SignOutButton } from "@clerk/react";
import { useMutation, useQuery } from "convex/react";
import { FormEvent, useState } from "react";
import { api } from "../convex/_generated/api";
import Badge from "./Badge";
import LoginPage from "./LoginPage";
import { useStoreUserEffect } from "./useStoreUserEffect";

export default function App() {
  const { isLoading, isAuthenticated } = useStoreUserEffect();
  return (
    <main>
      {isLoading ? (
        <>Loading...</>
      ) : isAuthenticated ? (
        <Content />
      ) : (
        <LoginPage />
      )}
    </main>
  );
}

function Content() {
  const messages = useQuery(api.messages.list) || [];

  const [newMessageText, setNewMessageText] = useState("");
  const sendMessage = useMutation(api.messages.send);

  async function handleSendMessage(event: FormEvent) {
    event.preventDefault();
    await sendMessage({ body: newMessageText });
    setNewMessageText("");
  }
  return (
    <main>
      <h1>Convex Chat</h1>
      <Badge />
      <h2>
        <SignOutButton />
      </h2>
      <ul>
        {messages.map((message) => (
          <li key={message._id}>
            <span>{message.author}:</span>
            <span>{message.body}</span>
            <span>{new Date(message._creationTime).toLocaleTimeString()}</span>
          </li>
        ))}
      </ul>
      <form onSubmit={handleSendMessage}>
        <input
          value={newMessageText}
          onChange={(event) => setNewMessageText(event.target.value)}
          placeholder="Write a message…"
        />
        <input type="submit" value="Send" disabled={newMessageText === ""} />
      </form>
    </main>
  );
}
