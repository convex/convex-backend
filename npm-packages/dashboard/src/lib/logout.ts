import posthog from "posthog-js";

export const LOGOUT_PATH = "/api/auth/logout?returnTo=/api/auth/login";

export function logout(url = LOGOUT_PATH): void {
  // Clear the PostHog person before another user logs in on this browser.
  posthog.reset();

  // Redirect to the logout page.
  window.location.href = url;
}
