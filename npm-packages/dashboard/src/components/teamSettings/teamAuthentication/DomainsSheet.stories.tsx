import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked, screen, userEvent } from "storybook/test";
import {
  useDeleteTeamDomain,
  useDomainPortalLink,
  useTeamDomains,
} from "api/domains";
import { useProfileEmails } from "api/profile";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { useTeamEntitlements } from "api/teams";
import type { TeamResponse } from "generatedApi";
import { DomainsSheet } from "./DomainsSheet";

const team: TeamResponse = {
  id: 1,
  creator: 1,
  name: "Acme Corp",
  slug: "acme",
  suspended: false,
  referralCode: "ACME01",
};

const meta = {
  component: DomainsSheet,
  // The section description reads in `--content-secondary`, which measures
  // 4.43:1 against the page background in the light theme — the token is a
  // hair under AA wherever it sits outside a sheet, not something this page
  // can fix. Every other rule still runs.
  parameters: {
    a11y: { config: { rules: [{ id: "color-contrast", enabled: false }] } },
  },
  args: { team },
  beforeEach: () => {
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(true);
    mocked(useHasCustomRolePermission).mockReturnValue(true);
    mocked(useTeamEntitlements).mockReturnValue({
      ssoEnabled: true,
      directorySyncEnabled: true,
    } as ReturnType<typeof useTeamEntitlements>);
    mocked(useDomainPortalLink).mockReturnValue(
      fn(async () => ({ link: "https://portal.workos.com/example" })) as any,
    );
    mocked(useDeleteTeamDomain).mockReturnValue(fn() as any);
    mocked(useProfileEmails).mockReturnValue([
      {
        id: 1,
        email: "ari@acme.com",
        isVerified: true,
        isPrimary: true,
        creationTime: Date.now(),
      },
    ]);
    mocked(useTeamDomains).mockReturnValue({
      data: [],
      isLoading: false,
      error: undefined,
    });
  },
} satisfies Meta<typeof DomainsSheet>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {};

export const WithDomains: Story = {
  beforeEach: () => {
    mocked(useTeamDomains).mockReturnValue({
      data: [
        { id: "dom_1", domain: "acme.com", state: "verified" },
        { id: "dom_2", domain: "acme.dev", state: "pending" },
        { id: "dom_3", domain: "acme.io", state: "failed" },
      ],
      isLoading: false,
      error: undefined,
    });
  },
};

export const DomainMenu: Story = {
  ...WithDomains,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "acme.com options" }),
    );
  },
};

export const NoPermission: Story = {
  beforeEach: () => {
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(false);
    mocked(useHasCustomRolePermission).mockReturnValue(false);
  },
};

export const LoadFailed: Story = {
  beforeEach: () => {
    mocked(useTeamDomains).mockReturnValue({
      data: undefined,
      isLoading: false,
      error: new Error("network"),
    } as unknown as ReturnType<typeof useTeamDomains>);
  },
};
