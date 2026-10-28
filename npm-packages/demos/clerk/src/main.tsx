import { ClerkProvider, useAuth } from "@clerk/react";
import { ConvexReactClient } from "convex/react";
import { ConvexProviderWithClerk } from "convex/react-clerk";
import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

const convex = new ConvexReactClient(import.meta.env.VITE_CONVEX_URL);
ReactDOM.createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ClerkProvider
      // Replace this with your Clerk Publishable Key
      // or with `{import.meta.env.VITE_CLERK_PUBLISHABLE_KEY}`
      // and configure VITE_CLERK_PUBLISHABLE_KEY in your .env.local
      publishableKey="pk_test_cm9idXN0LW1hZ2dvdC0yOS5jbGVyay5hY2NvdW50cy5kZXYk"
    >
      <ConvexProviderWithClerk client={convex} useAuth={useAuth}>
        <App />
      </ConvexProviderWithClerk>
    </ClerkProvider>
  </StrictMode>,
);
