import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked, screen, userEvent } from "storybook/test";
import {
  useDirectorySyncGroups,
  useDisableDirectorySync,
  useEnableDirectorySync,
  useGenerateDirectorySyncConfigurationLink,
  useGetDirectorySync,
  useSetGroupRoleMapping,
  useStagedDirectoryMembers,
} from "api/directorySync";
import {
  useDeleteTeamDomain,
  useDomainPortalLink,
  useTeamDomains,
} from "api/domains";
import { useProfileEmails } from "api/profile";
import { useListCustomRoles } from "api/roles";
import {
  useDisableSSO,
  useGenerateSSOConfigurationLink,
  useGetSSO,
  useUpdateSSO,
} from "api/teams";
import type {
  DirectoryGroupResponse,
  SsoOrganizationResponse,
  StagedDirectoryMemberResponse,
} from "generatedApi";
import { TeamAuthenticationPage } from "../../pages/t/[team]/settings/team-authentication";

const SSO_SHEET = '[data-testid="sso-sheet"]';
const DOMAINS_SHEET = '[data-testid="domains-sheet"]';
const DIRECTORY_SHEET = '[data-testid="directory-sync-sheet"]';
const DIRECTORY_GROUPS_SHEET = '[data-testid="directory-groups-sheet"]';
// The breadcrumb names the subpage, so it crops with it.
const REVIEW_PANE =
  '[data-testid="team-authentication-breadcrumb"], [data-testid="review-directory-changes"]';
const EDIT_GROUP_ROLE_POPOVER = '[data-testid="edit-group-role"]';

// The SSO docs don't cover Directory Sync, and their screenshots crop tightly
// enough that the section below still bleeds into the padding. Turning the flag
// off renders the page as a team with only SSO sees it.
const SSO_ONLY = { docsPage: { launchDarkly: { directorySync: false } } };

// The Directory Sync sections sit below the two above them, past the bottom of
// the default 700px capture viewport, and a crop only sees what the viewport
// rendered.
const TALL_VIEWPORT = { width: 1024, height: 1400 };

const verifiedDomain = {
  id: "dom_1",
  domain: "acme.dev",
  state: "verified",
} as const;

const noConnection: SsoOrganizationResponse = {
  createTime: new Date("2026-03-01T09:00:00Z").getTime(),
  requireSsoLogin: false,
  connections: [],
  domains: [verifiedDomain],
};

const withConnection: SsoOrganizationResponse = {
  ...noConnection,
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

const linkedDirectory = {
  id: "directory_1",
  name: "Acme Okta Directory",
  type: "okta scim v2.0",
  state: "linked",
  linked: true,
};

const groups: DirectoryGroupResponse[] = [
  {
    workosGroupId: "group_eng",
    name: "Engineering",
    idpId: "idp_eng",
    mapping: null,
  },
  {
    workosGroupId: "group_leads",
    name: "Engineering Leads",
    idpId: "idp_leads",
    mapping: { role: "admin" },
  },
  {
    workosGroupId: "group_support",
    name: "Support",
    idpId: "idp_support",
    mapping: {
      role: "custom",
      customRoles: [{ id: 7, name: "Support Engineer" }],
    },
  },
];

const stagedMembers: StagedDirectoryMemberResponse[] = [
  {
    member: {
      id: 1,
      name: "Dana",
      email: "dana@acme.dev",
      role: "admin",
    },
    directoryUser: {
      directoryUserId: "dir_user_1",
      email: "dana@acme.dev",
      state: "active",
      groups: [
        { workosGroupId: "group_leads", name: "Engineering Leads" },
        { workosGroupId: "group_eng", name: "Engineering" },
      ],
      role: "admin",
    },
  },
  {
    member: {
      id: 2,
      name: "Marcus",
      email: "marcus@acme.dev",
      role: "admin",
    },
    directoryUser: {
      directoryUserId: "dir_user_2",
      email: "marcus@acme.dev",
      state: "active",
      groups: [{ workosGroupId: "group_eng", name: "Engineering" }],
      role: "developer",
    },
  },
  {
    member: {
      id: 3,
      name: "Priya",
      email: "priya@acme.dev",
      role: "developer",
    },
    directoryUser: {
      directoryUserId: "dir_user_3",
      email: "priya@acme.dev",
      state: "suspended",
      groups: [{ workosGroupId: "group_eng", name: "Engineering" }],
      role: "developer",
    },
  },
  {
    directoryUser: {
      directoryUserId: "dir_user_4",
      email: "jordan@acme.dev",
      state: "active",
      groups: [{ workosGroupId: "group_support", name: "Support" }],
      role: "custom",
      customRoles: [{ id: 7, name: "Support Engineer" }],
    },
  },
  {
    member: {
      id: 5,
      name: "Contractor Account",
      email: "contractor@example.com",
      role: "developer",
    },
  },
];

function mockDirectory({
  directory,
  enabled = false,
  mirrored = true,
}: {
  directory: typeof linkedDirectory | null;
  enabled?: boolean;
  mirrored?: boolean;
}) {
  mocked(useGetDirectorySync).mockReturnValue({
    data: { directory, enabled, mirrored },
    isLoading: false,
    error: undefined,
  });
}

function mockGroups(data: DirectoryGroupResponse[]) {
  mocked(useDirectorySyncGroups).mockReturnValue({
    data: { groups: data, pagination: { hasMore: false, nextCursor: null } },
    isLoading: false,
    error: undefined,
  });
}

const meta = {
  component: TeamAuthenticationPage,
  parameters: {
    layout: "fullscreen",
    a11y: { test: "todo" },
    docsPage: {
      launchDarkly: { directorySync: true },
      entitlements: {
        ssoEnabled: true,
        directorySyncEnabled: true,
        customRolesEnabled: true,
      },
    },
  },
  beforeEach: () => {
    mocked(useDomainPortalLink).mockReturnValue(
      fn(async () => ({ link: "https://portal.workos.com/example" })) as any,
    );
    mocked(useDeleteTeamDomain).mockReturnValue(fn() as any);
    mocked(useTeamDomains).mockReturnValue({
      data: [],
      isLoading: false,
      error: undefined,
    });
    mocked(useProfileEmails).mockReturnValue([
      {
        id: 1,
        email: "nicolas@acme.dev",
        isVerified: true,
        isPrimary: true,
        creationTime: new Date("2026-01-05T09:00:00Z").getTime(),
      },
    ]);

    mocked(useGetSSO).mockReturnValue({
      data: { ...noConnection, domains: [] },
      isLoading: false,
      error: undefined,
    });
    mocked(useGenerateSSOConfigurationLink).mockReturnValue(
      fn(async () => ({ link: "https://portal.workos.com/example" })) as any,
    );
    mocked(useDisableSSO).mockReturnValue(fn() as any);
    mocked(useUpdateSSO).mockReturnValue(fn() as any);

    mockDirectory({ directory: null });
    mockGroups(groups);
    mocked(useStagedDirectoryMembers).mockReturnValue({
      data: {
        items: stagedMembers,
        pagination: { hasMore: false, nextCursor: null },
      },
      isLoading: false,
      error: undefined,
    });
    mocked(useGenerateDirectorySyncConfigurationLink).mockReturnValue(
      fn(async () => ({ link: "https://portal.workos.com/example" })) as any,
    );
    mocked(useEnableDirectorySync).mockReturnValue(fn() as any);
    mocked(useDisableDirectorySync).mockReturnValue(fn() as any);
    mocked(useSetGroupRoleMapping).mockReturnValue(fn() as any);
    mocked(useListCustomRoles).mockReturnValue({
      data: {
        items: [
          {
            id: 7,
            teamId: 2,
            name: "Support Engineer",
            statements: [],
            createTime: new Date("2026-02-01T09:00:00Z").getTime(),
          },
        ],
        pagination: { hasMore: false, nextCursor: null },
      },
    } as unknown as ReturnType<typeof useListCustomRoles>);
  },
} satisfies Meta<typeof TeamAuthenticationPage>;

export default meta;
type Story = StoryObj<typeof meta>;

/** The page a team lands on before anything has been set up. */
export const Default: Story = {};

function mockDomains() {
  mocked(useTeamDomains).mockReturnValue({
    data: [
      verifiedDomain,
      { id: "dom_2", domain: "acme.com", state: "pending" },
    ],
    isLoading: false,
    error: undefined,
  });
  mocked(useGetSSO).mockReturnValue({
    data: noConnection,
    isLoading: false,
    error: undefined,
  });
}

/** The page as a team that only has SSO sees it. */
export const SSOSections: Story = {
  parameters: {
    screenshotSelector: `${DOMAINS_SHEET}, ${SSO_SHEET}`,
    ...SSO_ONLY,
  },
};

/** A domain has to be verified before either product can be configured. */
export const Domains: Story = {
  parameters: { screenshotSelector: DOMAINS_SHEET, ...SSO_ONLY },
  beforeEach: mockDomains,
};

export const SingleSignOn: Story = {
  parameters: { screenshotSelector: SSO_SHEET, ...SSO_ONLY },
  beforeEach: mockDomains,
};

function mockSsoConfigured() {
  mockDomains();
  mocked(useGetSSO).mockReturnValue({
    data: withConnection,
    isLoading: false,
    error: undefined,
  });
}

export const SingleSignOnConfigured: Story = {
  parameters: { screenshotSelector: SSO_SHEET, ...SSO_ONLY },
  beforeEach: mockSsoConfigured,
};

export const SingleSignOnMenu: Story = {
  parameters: {
    screenshotSelector: `${SSO_SHEET}, [role="menu"]`,
    ...SSO_ONLY,
  },
  beforeEach: mockSsoConfigured,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Okta SAML options" }),
    );
  },
};

export const RequireSSO: Story = {
  parameters: { screenshotSelector: '[role="dialog"]', ...SSO_ONLY },
  beforeEach: mockSsoConfigured,
  play: async () => {
    // The design-system Checkbox carries its own aria-label.
    await userEvent.click(
      await screen.findByRole("checkbox", { name: "Selected" }),
    );
  },
};

export const DirectorySync: Story = {
  parameters: {
    screenshotSelector: DIRECTORY_SHEET,
    screenshotViewport: TALL_VIEWPORT,
  },
  beforeEach: mockSsoConfigured,
};

/** What "Configure" explains before handing the member off to the portal. */
export const DirectorySyncConfigure: Story = {
  parameters: { screenshotSelector: '[role="dialog"]' },
  beforeEach: mockSsoConfigured,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Configure" }),
    );
  },
};

/** The window after linking, before the identity provider sends the roster. */
export const DirectorySyncInitialSync: Story = {
  parameters: {
    screenshotSelector: `${DIRECTORY_SHEET}, ${DIRECTORY_GROUPS_SHEET}`,
    screenshotViewport: TALL_VIEWPORT,
  },
  beforeEach: () => {
    mockSsoConfigured();
    mockDirectory({ directory: linkedDirectory, mirrored: false });
    mockGroups([]);
  },
};

function mockDirectorySynced() {
  mockSsoConfigured();
  mockDirectory({ directory: linkedDirectory });
}

/** The synced directory, waiting on a review before it manages anyone. */
export const DirectorySyncSynced: Story = {
  parameters: {
    screenshotSelector: DIRECTORY_SHEET,
    screenshotViewport: TALL_VIEWPORT,
  },
  beforeEach: mockDirectorySynced,
};

export const DirectoryGroupRoles: Story = {
  parameters: {
    screenshotSelector: DIRECTORY_GROUPS_SHEET,
    screenshotViewport: TALL_VIEWPORT,
  },
  beforeEach: mockDirectorySynced,
};

export const EditGroupRole: Story = {
  parameters: { screenshotSelector: EDIT_GROUP_ROLE_POPOVER },
  beforeEach: mockDirectorySynced,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Edit Engineering role" }),
    );
  },
};

// The roster is a subpage of team authentication, so the story opens it by
// the URL that the Review button navigates to.
const REVIEW_SUBPAGE = {
  nextjs: {
    router: {
      pathname: "/t/[team]/settings/team-authentication",
      route: "/t/[team]/settings/team-authentication",
      asPath: "/t/acme/settings/team-authentication?review=1",
      query: { team: "acme", review: "1" },
    },
  },
};

export const ReviewDirectoryChanges: Story = {
  parameters: {
    ...REVIEW_SUBPAGE,
    screenshotSelector: REVIEW_PANE,
    // The roster is wider than the page's own content, and the subpage grows
    // with it.
    screenshotViewport: { width: 1280, height: 900 },
  },
  beforeEach: mockDirectorySynced,
};

export const DirectorySyncEnabled: Story = {
  parameters: {
    screenshotSelector: DIRECTORY_SHEET,
    screenshotViewport: TALL_VIEWPORT,
  },
  beforeEach: () => {
    mockSsoConfigured();
    mockDirectory({ directory: linkedDirectory, enabled: true });
  },
};

/** Who the directory covers that has not joined the team yet. */
export const PendingMembers: Story = {
  parameters: { ...REVIEW_SUBPAGE, screenshotSelector: REVIEW_PANE },
  beforeEach: () => {
    mockSsoConfigured();
    mockDirectory({ directory: linkedDirectory, enabled: true });
    mocked(useStagedDirectoryMembers).mockReturnValue({
      data: {
        items: stagedMembers.filter((item) => !item.member),
        pagination: { hasMore: false, nextCursor: null },
      },
      isLoading: false,
      error: undefined,
    });
  },
};
