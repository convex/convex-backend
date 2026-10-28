import "./preview.css";
import { Preview } from "@storybook/nextjs";

import themeDecorator from "./themeDecorator";
import { docsPageDecorator } from "./docsPageDecorator";
import { commandPaletteDecorator } from "./commandPaletteDecorator";
import { configure, sb } from "storybook/test";

// Register modules for mocking in stories
// Note: paths must be relative to this file and include extensions for Node.js resolution
sb.mock(import("dashboard/src/api/teams.ts"), { spy: true });
sb.mock(import("dashboard/src/api/projects.ts"), { spy: true });
sb.mock(import("dashboard/src/api/profile.ts"), { spy: true });
sb.mock(import("dashboard/src/api/deployments.ts"), { spy: true });
sb.mock(import("dashboard/src/api/roles.ts"), { spy: true });
sb.mock(import("dashboard/src/api/directorySync.ts"), { spy: true });
sb.mock(import("dashboard/src/api/domains.ts"), { spy: true });
sb.mock(import("dashboard/src/hooks/useDeploymentPermissions.ts"), {
  spy: true,
});
sb.mock(import("dashboard/src/api/invitations.ts"), { spy: true });
sb.mock(import("dashboard/src/api/billing.ts"), { spy: true });
sb.mock(import("dashboard/src/api/accessTokens.ts"), { spy: true });
sb.mock(import("dashboard/src/api/teamAccessTokens.ts"), { spy: true });
sb.mock(import("dashboard/src/api/personalAccessTokens.ts"), {
  spy: true,
});
sb.mock(import("dashboard/src/api/discord.ts"), { spy: true });
sb.mock(import("dashboard/src/api/backups.ts"), { spy: true });
sb.mock(import("dashboard/src/api/vanityDomains.ts"), { spy: true });
sb.mock(import("dashboard/src/api/environmentVariables.ts"), {
  spy: true,
});
sb.mock(import("dashboard/src/api/optins.ts"), { spy: true });
sb.mock(import("dashboard/src/api/api.ts"), { spy: true });
sb.mock(import("dashboard/src/hooks/useLaunchDarkly.tsx"), { spy: true });
sb.mock(import("dashboard/src/hooks/usePostHog.ts"), { spy: true });
sb.mock(import("dashboard/src/hooks/useServerSideData.ts"), {
  spy: true,
});
sb.mock(import("dashboard/src/api/usage.ts"), { spy: true });
sb.mock(import("dashboard/src/api/auditLog.ts"), { spy: true });
sb.mock(import("dashboard/src/api/oauth.ts"), { spy: true });
sb.mock(import("dashboard/src/api/referrals.ts"), { spy: true });
sb.mock(import("dashboard/src/lib/deploymentAuth.ts"), { spy: true });
sb.mock(import("dashboard/src/hooks/usageMetrics.ts"), { spy: true });
sb.mock(import("dashboard-common/src/elements/LocalDevCallout.tsx"), {
  spy: true,
});
sb.mock(
  import("dashboard-common/src/features/disconnectOverlay/CloudDisconnectOverlay.tsx"),
  {
    spy: true,
  },
);
sb.mock(import("dashboard/src/components/projectSettings/CustomDomains.tsx"), {
  spy: true,
});
sb.mock(import("dashboard-common/src/lib/deploymentApi.ts"), {
  spy: true,
});
sb.mock(import("dashboard-common/src/lib/appMetrics.ts"), {
  spy: true,
});
sb.mock(import("dashboard-common/src/features/settings/lib/api.ts"), {
  spy: true,
});
sb.mock(import("dashboard/src/hooks/useStripe.ts"));
// Do not mock dashboard-common/src/lib/deploymentContext.tsx: the mocked
// namespace hands out copies of the React context objects it defines, so a
// `<DeploymentInfoContext.Provider>` set up by a story or decorator is invisible
// to that module's own consumers (PermissionsProvider then reads `undefined` and
// every deployment page story fails to render). Stories that need a connected
// deployment provide MaybeConnectedDeploymentContext instead.

// Chromatic's capture machines are slower than a dev machine or CI runner, and
// page stories wait on mocked data that renders through a portal. The 1s
// testing-library default times out there even though the element does appear.
configure({ asyncUtilTimeout: 10_000 });

const preview: Preview = {
  initialGlobals: {
    a11y: { manual: false },
  },
  parameters: {
    a11y: { test: "error" },
    actions: { argTypesRegex: "^on[A-Z].*" },
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/,
      },
    },
  },

  decorators: [
    commandPaletteDecorator,
    docsPageDecorator,
    themeDecorator({
      themes: {
        light: "light",
        dark: "dark",
      },
      defaultTheme: "light",
    }),
  ],
};

export default preview;
