"use client";

import {
  AuthRefreshing,
  Authenticated,
  Unauthenticated,
  useConvex,
  useConvexAuth,
  useMutation,
  useQuery,
} from "convex/react";
import { api } from "../convex/_generated/api";
import { useAuthActions } from "@convex-dev/auth/react";
import { useState } from "react";
import "./styles.css";

export default function App() {
  return (
    <div className="container">
      <AuthRefreshing>
        <div className="refresh-banner">Refreshing session…</div>
      </AuthRefreshing>
      <header className="header">
        <h1>Convex + React + Convex Auth</h1>
        <SignOutButton />
      </header>
      <main>
        <Authenticated>
          <Content />
        </Authenticated>
        <Unauthenticated>
          <SignInForm />
        </Unauthenticated>
      </main>
    </div>
  );
}

function ForceReauthButton() {
  const convex = useConvex();
  const handleClick = () => {
    const authMgr = (
      convex as unknown as {
        sync: { authenticationManager: { onAuthError: (e: unknown) => void } };
      }
    ).sync.authenticationManager;
    authMgr.onAuthError({
      type: "AuthError",
      error: "test reauth (forced from UI)",
      baseVersion: 1_000_000,
      authUpdateAttempted: false,
    });
  };
  return (
    <button className="btn btn-secondary" onClick={handleClick}>
      Force reauth
    </button>
  );
}

function SignOutButton() {
  const { isAuthenticated } = useConvexAuth();
  const { signOut } = useAuthActions();
  return (
    <>
      {isAuthenticated && (
        <button className="btn btn-secondary" onClick={() => void signOut()}>
          Sign out
        </button>
      )}
    </>
  );
}

function SignInForm() {
  const { signIn } = useAuthActions();
  const [flow, setFlow] = useState<"signIn" | "signUp">("signIn");
  const [error, setError] = useState<string | null>(null);
  return (
    <div className="form">
      <p>Log in to use the chat</p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          const formData = new FormData(e.target as HTMLFormElement);
          formData.set("flow", flow);
          void signIn("password", formData).catch((error) => {
            setError(error.message);
          });
        }}
      >
        <div className="form-group">
          <input type="email" name="email" placeholder="Email" />
        </div>
        <div className="form-group">
          <input type="password" name="password" placeholder="Password" />
        </div>
        <button className="btn btn-primary" type="submit">
          {flow === "signIn" ? "Sign in" : "Sign up"}
        </button>
        <div className="form-footer">
          <span>
            {flow === "signIn"
              ? "Don't have an account?"
              : "Already have an account?"}
          </span>{" "}
          <a
            href="#"
            onClick={(e) => {
              e.preventDefault();
              setFlow(flow === "signIn" ? "signUp" : "signIn");
            }}
          >
            {flow === "signIn" ? "Sign up instead" : "Sign in instead"}
          </a>
        </div>
        {error && (
          <div className="error">
            <p>Error signing in: {error}</p>
          </div>
        )}
      </form>
    </div>
  );
}

function Content() {
  const messages = useQuery(api.chat.listMessages);
  const sendMessage = useMutation(api.chat.sendMessage);
  const [username, setUsername] = useState("");
  const [message, setMessage] = useState("");

  if (messages === undefined) {
    return (
      <div className="loading">
        <p>loading...</p>
      </div>
    );
  }

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (username.trim() && message.trim()) {
      void sendMessage({ username, message });
      setMessage("");
    }
  };

  return (
    <div>
      <div className="reauth-controls">
        <ForceReauthButton />
      </div>
      <div className="messages-container">
        {messages.length === 0 ? (
          <div className="empty-state">
            <p>No messages yet. Be the first to chat!</p>
          </div>
        ) : (
          messages.map((msg) => (
            <div key={msg._id} className="message-card">
              <strong>{msg.username}</strong>
              <p>{msg.message}</p>
            </div>
          ))
        )}
      </div>
      <form className="chat-form" onSubmit={handleSubmit}>
        <input
          type="text"
          placeholder="Username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
        />
        <input
          type="text"
          placeholder="Type your message..."
          value={message}
          onChange={(e) => setMessage(e.target.value)}
        />
        <button className="btn btn-primary" type="submit">
          Send
        </button>
      </form>
    </div>
  );
}
