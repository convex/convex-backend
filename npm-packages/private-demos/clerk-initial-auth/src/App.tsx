import { SignOutButton, useAuth } from "@clerk/react";
import { useMutation, useQuery } from "convex/react";
import { FormEvent, useState } from "react";
import { api } from "../convex/_generated/api";
import Badge from "./Badge";
import LoginPage from "./LoginPage";

export default function App() {
  const { isSignedIn, isLoaded } = useAuth();
  if (!isLoaded) {
    return <div>Clerk is loading...</div>;
  }
  return <main>{isSignedIn ? <Content /> : <LoginPage />}</main>;
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
    <>
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
    </>
  );
}
