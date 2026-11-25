import { Meta, StoryObj } from "@storybook/nextjs";
import { expect, fn, mocked, userEvent, within } from "storybook/test";
import { useDeletedProjects, useRecoverProject } from "api/projects";
import { useIsCurrentMemberTeamAdmin } from "api/roles";
import { flagDefaults, useLaunchDarkly } from "hooks/useLaunchDarkly";
import { RecentlyDeletedProjects } from "./RecentlyDeletedProjects";

const now = Date.now();
const day = 24 * 60 * 60 * 1000;

const meta = {
  component: RecentlyDeletedProjects,
  args: {
    team: {
      id: 1,
      slug: "acme",
      name: "Acme",
      creator: 1,
      suspended: false,
      referralCode: "ACME123",
      referredBy: null,
    },
  },
  beforeEach: () => {
    mocked(useLaunchDarkly).mockReturnValue({
      ...flagDefaults,
      projectRecovery: true,
    });
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(true);
    mocked(useRecoverProject).mockReturnValue(fn());
    mocked(useDeletedProjects).mockReturnValue({
      data: {
        items: [
          {
            id: 1,
            name: "Customer Portal",
            slug: "customer-portal",
            deletedAt: now - day,
            recoverableUntil: now + 6 * day,
          },
          {
            id: 2,
            name: "Inventory",
            slug: "inventory",
            deletedAt: now - 3 * day,
            recoverableUntil: now + 4 * day,
          },
        ],
        pagination: { hasMore: false },
      },
      hasError: false,
      isLoading: false,
    });
  },
} satisfies Meta<typeof RecentlyDeletedProjects>;

export default meta;
type Story = StoryObj<typeof meta>;

export const RecentlyDeleted: Story = {};

export const ListRequestFailed: Story = {
  beforeEach: () => {
    mocked(useDeletedProjects).mockReturnValue({
      data: undefined,
      hasError: true,
      isLoading: false,
    });
  },
  play: async ({ canvasElement }) => {
    await expect(within(canvasElement).getByRole("status")).toHaveTextContent(
      "Unable to load deleted projects.",
    );
  },
};

export const ConfirmRecover: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await within(canvas.getAllByRole("listitem")[0]).findByRole("button", {
        name: "Recover",
      }),
    );
    await within(document.body).findByRole("dialog", {
      name: "Recover Customer Portal?",
    });
  },
};
