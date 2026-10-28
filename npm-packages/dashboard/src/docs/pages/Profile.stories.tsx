import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, fn } from "storybook/test";
import {
  useDeleteAccount,
  useMfaStatus,
  useProfileEmails,
  useUpdateProfileName,
} from "api/profile";
import {
  useCreatePersonalAccessToken,
  useDeletePersonalAccessToken,
  usePaginatedPersonalAccessTokens,
} from "api/personalAccessTokens";
import { useDiscordAccounts, useUnlinkDiscordAccount } from "api/discord";
import { useDirectorySyncOffers } from "api/directorySync";
import { Profile } from "../../pages/profile";

const now = Date.now();
const oneDay = 86_400_000;

const meta = {
  component: Profile,
  parameters: {
    layout: "fullscreen",
    a11y: {
      test: "todo",
    },
  },
  beforeEach: () => {
    mocked(useProfileEmails).mockReturnValue([
      {
        id: 1,
        email: "nicolas@acme.dev",
        isPrimary: true,
        isVerified: true,
        creationTime: now - 365 * oneDay,
      },
      {
        id: 2,
        email: "nicolas.personal@example.com",
        isPrimary: false,
        isVerified: true,
        creationTime: now - 30 * oneDay,
      },
    ]);
    mocked(useUpdateProfileName).mockReturnValue(fn());
    mocked(useDeleteAccount).mockReturnValue(fn());
    mocked(useMfaStatus).mockReturnValue({ enabled: false });
    mocked(usePaginatedPersonalAccessTokens).mockReturnValue({
      data: {
        items: [
          {
            id: 1,
            name: "ci-deploy",
            creationTime: now - 30 * oneDay,
            lastUsedTime: now - 2 * oneDay,
          },
          {
            id: 2,
            name: "local-dev",
            creationTime: now - 7 * oneDay,
            lastUsedTime: null,
          },
        ],
        pagination: { hasMore: false },
      },
      isLoading: false,
    });
    mocked(useCreatePersonalAccessToken).mockReturnValue(fn());
    mocked(useDeletePersonalAccessToken).mockReturnValue(fn());
    mocked(useDiscordAccounts).mockReturnValue([]);
    mocked(useUnlinkDiscordAccount).mockReturnValue(fn());
    mocked(useDirectorySyncOffers).mockReturnValue([]);
  },
} satisfies Meta<typeof Profile>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

// Teams whose directory lists one of your verified emails. Directory Sync
// offers a team rather than adding you to it, so this is where a provisioned
// member accepts.
export const AvailableTeams: Story = {
  parameters: {
    screenshotSelector: "#available-teams",
    // The section sits below Profile information and Emails, past the bottom
    // of the default 700px capture viewport.
    screenshotViewport: { width: 1024, height: 1100 },
  },
  beforeEach: () => {
    mocked(useDirectorySyncOffers).mockReturnValue([
      { teamId: 14, teamName: "Acme Corp", email: "nicolas@acme.dev" },
    ]);
  },
};
