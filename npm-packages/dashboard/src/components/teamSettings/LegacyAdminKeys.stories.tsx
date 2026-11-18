import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked, userEvent, within } from "storybook/test";
import { TeamResponse } from "generatedApi";
import {
  useDisableTeamLegacyAdminKeys,
  useTeamLegacyAdminKeyUsage,
} from "api/legacyAdminKeys";
import { useIsCurrentMemberTeamAdmin } from "api/roles";
import { flagDefaults, useLaunchDarkly } from "hooks/useLaunchDarkly";
import { LegacyAdminKeys } from "./LegacyAdminKeys";

const team: TeamResponse = {
  id: 1,
  slug: "acme",
  name: "Acme",
  creator: 1,
  suspended: false,
  referralCode: "ACME123",
  referredBy: null,
};

const now = Date.now();
const oneDay = 24 * 60 * 60 * 1000;

const meta = {
  component: LegacyAdminKeys,
  args: { team },
  beforeEach: () => {
    mocked(useLaunchDarkly).mockReturnValue({
      ...flagDefaults,
      showLegacyKeyUsage: true,
    });
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(true);
    mocked(useDisableTeamLegacyAdminKeys).mockReturnValue(fn());
    mocked(useTeamLegacyAdminKeyUsage).mockReturnValue({
      disabledTime: null,
      deployments: [
        {
          deploymentName: "happy-animal-123",
          lastUsedTime: now - 2 * 60_000,
        },
        {
          deploymentName: "brave-falcon-891",
          lastUsedTime: now - 3 * oneDay,
        },
        {
          deploymentName: "quiet-eagle-345",
          lastUsedTime: now - 45 * oneDay,
        },
      ],
    });
  },
} satisfies Meta<typeof LegacyAdminKeys>;

export default meta;
type Story = StoryObj<typeof meta>;

export const RecentlyUsed: Story = {};

export const ConfirmDisable: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: "Disable Legacy Admin Keys" }),
    );
  },
};

export const NeverUsed: Story = {
  beforeEach: () => {
    mocked(useTeamLegacyAdminKeyUsage).mockReturnValue({
      disabledTime: null,
      deployments: [],
    });
  },
};

export const Disabled: Story = {
  beforeEach: () => {
    mocked(useTeamLegacyAdminKeyUsage).mockReturnValue({
      disabledTime: now - 5 * oneDay,
      deployments: [
        { deploymentName: "brave-falcon-891", lastUsedTime: now - 6 * oneDay },
      ],
    });
  },
};

export const NotTeamAdmin: Story = {
  beforeEach: () => {
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(false);
  },
};

export const HiddenWithoutFlag: Story = {
  beforeEach: () => {
    mocked(useLaunchDarkly).mockReturnValue(flagDefaults);
  },
};

const animals = ["happy-animal", "brave-falcon", "quiet-eagle", "swift-otter"];

export const ManyDeployments: Story = {
  beforeEach: () => {
    mocked(useTeamLegacyAdminKeyUsage).mockReturnValue({
      disabledTime: null,
      deployments: Array.from({ length: 23 }, (_, i) => ({
        deploymentName: `${animals[i % animals.length]}-${100 + i}`,
        lastUsedTime: now - (i + 1) * 3 * 60 * 60 * 1000,
      })),
    });
  },
};
