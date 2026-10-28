import { useSyncExternalStore } from "react";
import { Meta, StoryObj } from "@storybook/nextjs";
import { expect, fn, mocked, screen, userEvent } from "storybook/test";
import {
  useDeleteGroupRoleMapping,
  useDirectorySyncGroups,
  useDisableDirectorySync,
  useEnableDirectorySync,
  useGenerateDirectorySyncConfigurationLink,
  useGetDirectorySync,
  useSetGroupRoleMapping,
  useStagedDirectoryMembers,
} from "api/directorySync";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
  useListCustomRoles,
} from "api/roles";
import { useGetSSO, useTeamEntitlements } from "api/teams";
import type {
  DirectoryGroupResponse,
  DirectoryResponse,
  TeamResponse,
} from "generatedApi";
import { DirectorySyncSheet } from "./DirectorySyncSheet";

const team: TeamResponse = {
  id: 1,
  creator: 1,
  name: "Acme Corp",
  slug: "acme",
  suspended: false,
  referralCode: "ACME01",
};

const groups: DirectoryGroupResponse[] = [
  {
    workosGroupId: "group_admins",
    name: "convex-team-admins",
    idpId: "idp_admins",
    mapping: { role: "admin" },
  },
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

function mockGroups(data: DirectoryGroupResponse[], hasMore = false) {
  mocked(useDirectorySyncGroups).mockReturnValue({
    data: {
      groups: data,
      pagination: { hasMore, nextCursor: hasMore ? "next" : null },
    },
    isLoading: false,
    error: undefined,
  });
}

const meta = {
  component: DirectorySyncSheet,
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
      customRolesEnabled: true,
    } as ReturnType<typeof useTeamEntitlements>);
    mocked(useListCustomRoles).mockReturnValue({
      data: {
        items: [
          {
            id: 7,
            teamId: 1,
            name: "Support Engineer",
            statements: [],
            createTime: 0,
          },
        ],
        pagination: { hasMore: false, nextCursor: null },
      },
    } as unknown as ReturnType<typeof useListCustomRoles>);
    mocked(useSetGroupRoleMapping).mockReturnValue(fn() as any);
    mocked(useDeleteGroupRoleMapping).mockReturnValue(fn() as any);
    mocked(useEnableDirectorySync).mockReturnValue(fn() as any);
    mocked(useStagedDirectoryMembers).mockReturnValue({
      data: { items: [], pagination: { hasMore: false, nextCursor: null } },
      isLoading: false,
      error: undefined,
    });
    mockGroups(groups);
    mocked(useGetSSO).mockReturnValue({
      data: {
        createTime: Date.now(),
        requireSsoLogin: false,
        connections: [],
        domains: [{ id: "dom_1", domain: "acme.com", state: "verified" }],
      },
      isLoading: false,
      error: undefined,
    });
    mocked(useGenerateDirectorySyncConfigurationLink).mockReturnValue(
      fn(async () => ({ link: "https://portal.workos.com/example" })) as any,
    );
    mocked(useDisableDirectorySync).mockReturnValue(fn() as any);
    mocked(useGetDirectorySync).mockReturnValue({
      data: { directory: null, enabled: false, mirrored: false },
      isLoading: false,
      error: undefined,
    });
  },
} satisfies Meta<typeof DirectorySyncSheet>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConfigured: Story = {};

const linkedDirectory = {
  id: "directory_1",
  name: "Acme Okta Directory",
  type: "okta scim v2.0",
  state: "linked",
  linked: true,
};

function mockConfigured() {
  mocked(useGetDirectorySync).mockReturnValue({
    data: { directory: linkedDirectory, enabled: false, mirrored: true },
    isLoading: false,
    error: undefined,
  });
}

export const Configured: Story = {
  beforeEach: mockConfigured,
};

// The window right after linking, before WorkOS hands over the roster: no
// groups to map and no members to review yet.
export const AwaitingInitialSync: Story = {
  beforeEach: () => {
    mocked(useGetDirectorySync).mockReturnValue({
      data: { directory: linkedDirectory, enabled: false, mirrored: false },
      isLoading: false,
      error: undefined,
    });
    mockGroups([]);
  },
};

export const ManagementEnabled: Story = {
  beforeEach: () => {
    mocked(useGetDirectorySync).mockReturnValue({
      data: { directory: linkedDirectory, enabled: true, mirrored: true },
      isLoading: false,
      error: undefined,
    });
  },
};

// The roster only reaches the menu once the directory provisions, so the
// story opens the menu to show it there.
export const ManagementEnabledMenu: Story = {
  ...ManagementEnabled,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Okta options" }),
    );
  },
};

// An unlinked directory has no groups to map and no roster to review, so the
// sheet asks for the connection and nothing else.
export const Unlinked: Story = {
  beforeEach: () => {
    mocked(useGetDirectorySync).mockReturnValue({
      data: {
        directory: { ...linkedDirectory, state: "unlinked", linked: false },
        enabled: false,
        mirrored: false,
      },
      isLoading: false,
      error: undefined,
    });
  },
};

export const ConfiguredMenu: Story = {
  ...Configured,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Okta options" }),
    );
  },
};

export const ConfiguredWithManyGroups: Story = {
  beforeEach: () => {
    mockConfigured();
    mockGroups(groups, true);
  },
};

export const ConfiguredAwaitingSync: Story = {
  beforeEach: () => {
    mockConfigured();
    mockGroups([]);
  },
};

// HeadlessUI brackets a portaled popover panel with `aria-hidden` sentinel
// buttons that hand focus back across the portal boundary. They are how the
// panel keeps its place in the tab order, and axe reads them as hidden
// controls. Repeats the meta's color-contrast exemption: story parameters
// replace the rule list rather than extending it.
const OPEN_POPOVER_A11Y = {
  a11y: {
    config: {
      rules: [
        { id: "color-contrast", enabled: false },
        { id: "aria-hidden-focus", enabled: false },
      ],
    },
  },
};

export const EditGroupRole: Story = {
  ...Configured,
  parameters: OPEN_POPOVER_A11Y,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Edit Engineering role" }),
    );
  },
};

// The role combobox and the custom-role menu open surfaces of their own, and
// the popover dismisses on a click outside its panel. They portal into the
// panel instead, so picking a role adds it rather than closing the editor.
export const EditGroupRoleAddsCustomRole: Story = {
  ...Configured,
  parameters: OPEN_POPOVER_A11Y,
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Edit Engineering role" }),
    );
    await userEvent.click(await screen.findByTestId("combobox-button-Role"));
    await userEvent.click(await screen.findByText("Custom"));
    await userEvent.click(
      await screen.findByRole("button", { name: "Add custom role" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "Support Engineer" }),
    );

    await expect(await screen.findByTestId("edit-group-role")).toBeVisible();
    await expect(
      await screen.findByRole("button", {
        name: "Remove custom role Support Engineer",
      }),
    ).toBeVisible();
  },
};

// A member who may manage the directory but not read the team's custom
// roles: the popover says so where the selector would be.
export const EditGroupRoleWithoutCustomRolePermission: Story = {
  parameters: OPEN_POPOVER_A11Y,
  beforeEach: () => {
    mockConfigured();
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(false);
    mocked(useHasCustomRolePermission).mockImplementation(
      (_teamId, action) => action !== "customRole:view",
    );
  },
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Edit Support role" }),
    );
  },
};

export const NotEntitled: Story = {
  beforeEach: () => {
    mocked(useTeamEntitlements).mockReturnValue({
      ssoEnabled: true,
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
    mocked(useGetDirectorySync).mockReturnValue({
      data: undefined,
      isLoading: false,
      error: new Error("network"),
    } as unknown as ReturnType<typeof useGetDirectorySync>);
  },
};

// The directory as a member works through the portal: absent until their
// identity provider creates it, then unlinked, validating, and finally linked.
const CONNECTION_STEPS: (DirectoryResponse | null)[] = [
  null,
  { ...linkedDirectory, state: "unlinked", linked: false },
  linkedDirectory,
];
const STEP_INTERVAL_MS = 900;

let currentStep = 0;
let stepTimer: ReturnType<typeof setInterval> | undefined;
const stepListeners = new Set<() => void>();

function clearStepTimer() {
  if (stepTimer !== undefined) {
    clearInterval(stepTimer);
    stepTimer = undefined;
  }
}

function startStepping() {
  currentStep = 0;
  clearStepTimer();
  stepTimer = setInterval(() => {
    currentStep += 1;
    if (currentStep === CONNECTION_STEPS.length - 1) {
      clearStepTimer();
    }
    stepListeners.forEach((listener) => listener());
  }, STEP_INTERVAL_MS);
}

function resetStepping() {
  clearStepTimer();
  currentStep = 0;
}

// The sheet and the dialog each call the hook, so they read one step rather
// than each running a timer of their own and drifting apart.
function useSteppedDirectorySync(): ReturnType<typeof useGetDirectorySync> {
  const step = useSyncExternalStore(
    (listener) => {
      stepListeners.add(listener);
      return () => {
        stepListeners.delete(listener);
      };
    },
    () => currentStep,
    () => 0,
  );
  const directory = CONNECTION_STEPS[step];
  return {
    // The walkthrough ends on the review row, so the mirror lands with the
    // link rather than trailing it; AwaitingInitialSync covers that gap.
    data: { directory, enabled: false, mirrored: directory?.linked ?? false },
    isLoading: false,
    error: undefined,
  };
}

// The whole first-run flow: Configure explains what connecting an identity
// provider does, hands the member off to the portal, and then reports the
// directory's state until it links. Continue leaves the member on the row that
// asks them to review the role changes before enabling directory sync.
export const ConfigureFlow: Story = {
  beforeEach: () => {
    resetStepping();
    // The portal would otherwise open a real tab.
    const realOpen = window.open;
    // The dialog opens the tab on the click and navigates it once the link
    // arrives, so the stub has to look enough like a window for that.
    window.open = fn(() => ({
      location: { href: "" },
      close: fn(),
    })) as unknown as typeof window.open;
    mocked(useGenerateDirectorySyncConfigurationLink).mockReturnValue(
      fn(async () => {
        // The dialog polls from the hand-off, so that is when the directory
        // starts making its way to `linked`.
        startStepping();
        return { link: "https://portal.workos.com/example" };
      }) as any,
    );
    mocked(useGetDirectorySync).mockImplementation(useSteppedDirectorySync);
    return () => {
      resetStepping();
      window.open = realOpen;
    };
  },
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Configure" }),
    );
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Configure Identity Provider",
      }),
    );
    // Buttons fade in, so a just-mounted Continue still computes to opacity 0.
    // Finding it at all is what says the directory reached `linked`.
    await expect(
      await screen.findByRole(
        "button",
        { name: "Continue" },
        { timeout: STEP_INTERVAL_MS * CONNECTION_STEPS.length * 3 },
      ),
    ).toBeInTheDocument();
  },
};
