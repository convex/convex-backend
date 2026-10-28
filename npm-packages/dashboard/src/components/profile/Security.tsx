// Third-party stylesheets (imported from node_modules, which Next.js permits
// outside _app). Scoped to this route since the widget only renders here.
import "@radix-ui/themes/styles.css";
import "@workos-inc/widgets/base.css";
import "@workos-inc/widgets/styles.css";

import { UserSecurity, UserSessions, WorkOsWidgets } from "@workos-inc/widgets";
import { Sheet } from "@ui/Sheet";
import { Callout } from "@ui/Callout";
import { useTheme } from "next-themes";
import { useWorkOS } from "hooks/useWorkOS";
import { useProfileEmails } from "api/profile";
import { PROFILE_SECTIONS } from "lib/sectionAnchors";

const FONT_FAMILY =
  "Inter Variable, Inter, ui-sans-serif, system-ui, sans-serif";

const WIDGET_CSS = `
.radix-themes.radix-themes {
  --default-font-family: ${FONT_FAMILY};
  /* The theme root otherwise forces min-height: 100vh, leaving a huge gap. */
  min-height: auto;
  /* Page background falls through to the surrounding Sheet. */
  --color-background: transparent;
  /* Panels/dialogs use the dashboard surface; cards sit one step raised. */
  --color-panel-solid: var(--background-secondary);
  --color-panel-translucent: var(--background-secondary);
  --color-surface: var(--background-tertiary);
  /* Solid accent (primary buttons, selected states). */
  --accent-9: var(--accent);
  --accent-10: var(--accent);
  --accent-contrast: rgb(255 255 255);
  /* Borders and text. */
  --gray-a6: var(--border-transparent);
  --gray-a7: var(--border-transparent);
  --gray-11: var(--content-secondary);
  --gray-12: var(--content-primary);
  /* WorkOS base-layer elements (text fields, selects, dropdowns, dialogs). */
  --woswidgets-accent-color: var(--accent);
  --woswidgets-border-color: var(--border-transparent);
  --woswidgets-background-color: var(--background-secondary);
  --woswidgets-foreground-color: var(--content-primary);
  /* Radix Themes defaults buttons to the arrow cursor. */
  --cursor-button: pointer;
}
/* Belt-and-suspenders for any widget buttons that aren't Radix buttons (and so
   don't read --cursor-button). Scoped to enabled buttons within the widget. */
.radix-themes button:not(:disabled) {
  cursor: pointer;
}
/* Match the widget text fields to the dashboard's TextInput. The widget
   defaults them to the browser's system \`field\`/\`fieldtext\`colors with no
   radius and an OS focus ring; point them at the dashboard surface, round the
   corners, and use the selected-border focus state instead. (The border color
   already comes from --woswidgets-border-color above.) */
.radix-themes .woswidgets-text-field {
  background-color: var(--background-secondary);
  color: var(--content-primary);
  border-radius: 0.375rem;
}
.radix-themes .woswidgets-text-field:focus-within {
  outline: none;
  border-color: var(--border-selected);
}
.radix-themes .woswidgets-text-field input {
  padding: 0.375rem 0.5rem;
  font-size: 0.875rem;
}
.radix-themes .woswidgets-text-field input::placeholder {
  color: var(--content-tertiary);
}
/* Radix Themes portals dialogs to the end of <body> without a z-index, which
   leaves them painted under the dashboard header (z-40) and the settings
   sidebar's scroll indicator (z-10). Match the design system's Popover. */
.radix-themes.rt-BaseDialogOverlay {
  z-index: 50;
}
/* The authenticator-enrollment QR code sits in a frame the widget paints with
   the theme's surface color, near-black in dark mode; its padding is the code's
   quiet zone, which cameras read most reliably in white. The widget sets the
   background inline, hence \`!important\`. \`secret-note\` is the id of the "Can't
   scan the code?" hint the image points at with aria-describedby. */
.radix-themes :has(> img[aria-describedby="secret-note"]) {
  background: #fff !important;
}
`;

// The widget calls the WorkOS API directly from the browser with a short-lived
// access token. Returning a function (rather than a static token) lets the
// widget fetch a freshly-refreshed token whenever it needs one; /api/auth/session
// refreshes the sealed session server-side before handing back the access token.
async function getWidgetAccessToken(): Promise<string> {
  const response = await fetch("/api/auth/session");
  if (!response.ok) {
    throw new Error("Could not load your session. Please sign in again.");
  }
  const session = await response.json();
  if (!session.accessToken) {
    throw new Error("Your session is missing an access token.");
  }
  return session.accessToken;
}

export function Security() {
  const { resolvedTheme } = useTheme();
  const { user } = useWorkOS();
  const emails = useProfileEmails();

  const primaryEmail = emails?.find((email) => email.isPrimary)?.email;
  const loggedInEmail = user?.email;
  const isPrimarySession =
    !!loggedInEmail &&
    !!primaryEmail &&
    loggedInEmail.toLowerCase() === primaryEmail.toLowerCase();

  return (
    <Sheet id={PROFILE_SECTIONS.security.id} className="flex flex-col gap-4">
      <h3>Security</h3>
      {isPrimarySession ? (
        <>
          <style dangerouslySetInnerHTML={{ __html: WIDGET_CSS }} />
          <WorkOsWidgets
            theme={{
              appearance: resolvedTheme === "dark" ? "dark" : "light",
              accentColor: "indigo",
              grayColor: "gray",
              panelBackground: "solid",
              radius: "medium",
              fontFamily: FONT_FAMILY,
            }}
          >
            <div className="flex flex-col gap-8">
              <UserSecurity authToken={getWidgetAccessToken} />
              <div className="flex flex-col gap-4">
                <h4 className="text-sm font-semibold text-content-primary">
                  Active sessions
                </h4>
                <UserSessions authToken={getWidgetAccessToken} />
              </div>
            </div>
          </WorkOsWidgets>
        </>
      ) : (
        <Callout>
          <div className="flex flex-col gap-2">
            {primaryEmail && loggedInEmail ? (
              <>
                <p>
                  You're signed in with <strong>{loggedInEmail}</strong>, which
                  is not your primary email.{" "}
                </p>
                <p>
                  To manage security settings, sign out and sign back in with
                  your primary email (<strong>{primaryEmail}</strong>), or
                  change your primary email address. Once you enable
                  multi-factor authentication, you will no longer be able to
                  sign in with secondary email addresses.
                </p>
              </>
            ) : (
              <span>
                Sign in with your primary email to manage security settings.
              </span>
            )}
          </div>
        </Callout>
      )}
    </Sheet>
  );
}
