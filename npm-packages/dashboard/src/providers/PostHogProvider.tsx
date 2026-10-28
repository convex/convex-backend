import { useProfile } from "api/profile";
import { useCurrentTeam } from "api/teams";
import posthog from "posthog-js";
import { PostHogProvider as PHProvider } from "posthog-js/react";
import React, { useEffect, useState } from "react";

export function PostHogProvider({
  children,
}: {
  children: React.ReactElement;
}) {
  const profile = useProfile();
  const team = useCurrentTeam();
  const [postHogReady, setPostHogReady] = useState(() => posthog.__loaded);

  useEffect(() => {
    const isProduction = process.env.NODE_ENV === "production";
    const isDebugMode = process.env.NEXT_PUBLIC_POSTHOG_DEBUG === "true";
    // Public 'Project API Key' for PostHog, safe to expose.
    const key = process.env.NEXT_PUBLIC_POSTHOG_KEY;
    const api_host = process.env.NEXT_PUBLIC_POSTHOG_HOST;

    if (
      (!isProduction && !isDebugMode) ||
      !key ||
      !api_host ||
      posthog.__loaded
    ) {
      return;
    }

    // See https://posthog.com/docs/libraries/js#config
    posthog.init(key, {
      api_host,
      ui_host: "https://us.posthog.com/",
      // Logs event details to the console.
      debug: isDebugMode,
      // Capture pageviews for initial load and client-side route changes.
      // See https://posthog.com/tutorials/single-page-app-pageviews
      capture_pageview: "history_change",
      session_recording: {
        recordHeaders: false,
        maskTextSelector: "*", // Masks all text elements (not including inputs)
        maskAllInputs: true, // Masks all input elements
      },
      loaded() {
        setPostHogReady(true);
      },
    });
  }, []);

  // Associates events with the selected team ("company" in PostHog).
  useEffect(() => {
    if (!team || !postHogReady) {
      return;
    }

    posthog.group("company", team.id.toString(), {
      name: team.name,
      slug: team.slug,
    });
  }, [team, postHogReady]);

  // Associates events with the user. We wait for PostHog to read the shared
  // subdomain cookie first, which may contain an anonymous ID from an earlier
  // visit to the website, docs, Stack, etc. Calling identify() before that
  // loads would skip the merge and we'd lose $anon_distinct_id on this person.
  useEffect(() => {
    if (!profile || !postHogReady) {
      return;
    }

    const profileId = profile.id.toString();
    if (posthog.get_distinct_id() !== profileId) {
      posthog.identify(profileId);
    }
  }, [profile, postHogReady]);

  return <PHProvider client={posthog}>{children}</PHProvider>;
}
