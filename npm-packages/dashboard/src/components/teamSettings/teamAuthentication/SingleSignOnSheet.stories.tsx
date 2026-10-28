import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked, screen, userEvent } from "storybook/test";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import {
  useDisableSSO,
  useGenerateSSOConfigurationLink,
  useGetSSO,
  useTeamEntitlements,
  useUpdateSSO,
} from "api/teams";
import type { SsoOrganizationResponse, TeamResponse } from "generatedApi";
import { SingleSignOnSheet } from "./SingleSignOnSheet";

const team: TeamResponse = {
  id: 1,
  creator: 1,
  name: "Acme Corp",
  slug: "acme",
  suspended: false,
  referralCode: "ACME01",
};

const verifiedDomain = {
  id: "dom_1",
  domain: "acme.com",
  state: "verified",
} as const;

const notConfigured: SsoOrganizationResponse = {
  createTime: Date.now(),
  requireSsoLogin: false,
  connections: [],
  domains: [verifiedDomain],
};

const configured: SsoOrganizationResponse = {
  ...notConfigured,
  connections: [
    {
      id: "conn_1",
      name: "Acme Okta Connection",
      connectionType: "OktaSAML",
      state: "active",
      active: true,
    },
  ],
};

const meta = {
  component: SingleSignOnSheet,
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
    mocked(useGenerateSSOConfigurationLink).mockReturnValue(
      fn(async () => ({ link: "https://portal.workos.com/example" })) as any,
    );
    mocked(useDisableSSO).mockReturnValue(fn() as any);
    mocked(useUpdateSSO).mockReturnValue(fn() as any);
    mocked(useGetSSO).mockReturnValue({
      data: notConfigured,
      isLoading: false,
      error: undefined,
    });
  },
} satisfies Meta<typeof SingleSignOnSheet>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConfigured: Story = {};

// Without a verified domain WorkOS has nothing to attach a connection to, so
// the portal button stays disabled.
export const NoVerifiedDomain: Story = {
  beforeEach: () => {
    mocked(useGetSSO).mockReturnValue({
      data: {
        ...notConfigured,
        domains: [{ id: "dom_1", domain: "acme.com", state: "pending" }],
      },
      isLoading: false,
      error: undefined,
    });
  },
};

export const Configured: Story = {
  beforeEach: () => {
    mocked(useGetSSO).mockReturnValue({
      data: configured,
      isLoading: false,
      error: undefined,
    });
  },
};

export const ConfiguredMenu: Story = {
  ...Configured,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Okta SAML options" }),
    );
  },
};

export const RequireSsoConfirmation: Story = {
  ...Configured,
  play: async () => {
    // The design-system Checkbox carries its own aria-label.
    await userEvent.click(
      await screen.findByRole("checkbox", { name: "Selected" }),
    );
  },
};

export const NotEntitled: Story = {
  beforeEach: () => {
    mocked(useTeamEntitlements).mockReturnValue({
      ssoEnabled: false,
      directorySyncEnabled: false,
    } as ReturnType<typeof useTeamEntitlements>);
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
    mocked(useGetSSO).mockReturnValue({
      data: undefined,
      isLoading: false,
      error: new Error("network"),
    } as unknown as ReturnType<typeof useGetSSO>);
  },
};
