import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, screen, userEvent } from "storybook/test";
import React from "react";
import { flagDefaults, useLaunchDarkly } from "hooks/useLaunchDarkly";
import { useCurrentTeam, useTeams } from "api/teams";
import {
  useCurrentProject,
  useInfiniteProjects,
  useProjectById,
} from "api/projects";
import { useDeployments, useInfiniteDeployments } from "api/deployments";
import { useProfile } from "api/profile";
import { useHasCustomRolePermission } from "api/roles";
import { CommandPalette } from "elements/CommandPalette";
import {
  useDirectorySyncOffers,
  useJoinDirectorySyncedTeam,
} from "api/directorySync";
import { JoinDirectorySyncedTeamModal } from "components/header/JoinDirectorySyncedTeamModal";
import { ignoredDirectorySyncTeamsKey } from "hooks/useIgnoredDirectorySyncTeams";
import type {
  DirectorySyncOffer,
  PlatformDeploymentResponse,
} from "generatedApi";
import { ProjectSelector } from "./ProjectSelector";

const mockTeam = {
  id: 2,
  creator: 1,
  slug: "acme",
  name: "Acme Corp",
  suspended: false,
  referralCode: "ACME01",
  referredBy: null,
};

const mockProject = {
  id: 7,
  teamId: mockTeam.id,
  name: "My amazing app",
  slug: "my-amazing-app",
  createTime: Date.now(),
  prodDeploymentName: "musical-otter-456",
  devDeploymentName: "happy-capybara-123",
} as NonNullable<ReturnType<typeof useCurrentProject>>;

const otherProjects = [
  { ...mockProject },
  {
    id: 8,
    teamId: mockTeam.id,
    name: "Marketing site",
    slug: "marketing-site",
    createTime: Date.now(),
  },
  {
    id: 9,
    teamId: mockTeam.id,
    name: "Internal tools",
    slug: "internal-tools",
    createTime: Date.now(),
  },
] as NonNullable<ReturnType<typeof useCurrentProject>>[];

const devDeployment: PlatformDeploymentResponse = {
  id: 11,
  name: "happy-capybara-123",
  deploymentType: "dev",
  kind: "cloud",
  isDefault: true,
  projectId: mockProject.id,
  creator: 1,
  createTime: 0,
  class: "s256",
  deploymentUrl: "https://happy-capybara-123.convex.cloud",
  reference: "dev/nicolas",
  region: "aws-us-east-1",
};

const mockProfile = {
  id: 1,
  name: "Nicolas Ettlin",
  email: "nicolas@acme.dev",
};

// A team the member isn't in, offered because its directory lists one of their
// verified emails.
const directorySyncOffer: DirectorySyncOffer = {
  teamId: 14,
  teamName: "Example Org",
  email: "nicolas@example.org",
};

// The selector opens the palette anchored beneath whichever segment was
// clicked, so a story needs the palette mounted alongside the trigger. Wrap
// both in a header-like bar to place the trigger where it lives in the app.
function ProjectSelectorHarness(
  props: React.ComponentProps<typeof ProjectSelector>,
) {
  return (
    <div
      className="flex h-14 items-center border-b bg-background-secondary px-3"
      style={
        {
          "--project-selector-bg": "var(--background-primary)",
        } as React.CSSProperties
      }
    >
      <ProjectSelector {...props} />
      <CommandPalette />
      {/* Mounted at the app root in `_app`; the team switcher opens it by
          setting the shared offer state. */}
      <JoinDirectorySyncedTeamModal />
    </div>
  );
}

const meta = {
  component: ProjectSelector,
  parameters: {
    layout: "fullscreen",
    // The anchored menu is a focus-trapping Radix dialog over an otherwise
    // empty canvas, which trips the automated a11y checks meant for full pages.
    a11y: { test: "todo" },
    nextjs: {
      router: {
        pathname: "/t/[team]",
        route: "/t/[team]",
        asPath: "/t/acme",
        query: { team: "acme" },
      },
    },
  },
  render: (args) => <ProjectSelectorHarness {...args} />,
  beforeEach: () => {
    mocked(useLaunchDarkly).mockReturnValue(flagDefaults);
    mocked(useTeams).mockReturnValue({
      selectedTeamSlug: mockTeam.slug,
      teams: [mockTeam],
    });
    mocked(useCurrentTeam).mockReturnValue(mockTeam);
    mocked(useCurrentProject).mockReturnValue(mockProject);
    mocked(useProfile).mockReturnValue(mockProfile);
    mocked(useHasCustomRolePermission).mockReturnValue(true);
    mocked(useDirectorySyncOffers).mockReturnValue([]);
    // Ignoring an offer persists to localStorage, which the test runner shares
    // across stories; reset it so the switcher still lists the offer.
    window.localStorage.removeItem(
      ignoredDirectorySyncTeamsKey(mockProfile.id),
    );
    mocked(useJoinDirectorySyncedTeam).mockReturnValue(async () => ({
      teamId: directorySyncOffer.teamId,
      teamSlug: "example-org",
    }));
    mocked(useProjectById).mockReturnValue({
      project: mockProject,
      isLoading: false,
      error: undefined,
    });
    // Server-backed search: the remote rows bypass the palette's client-side
    // filter, so filter the mock data by the search argument to mirror it.
    mocked(useInfiniteProjects).mockImplementation(
      (_teamId, searchQuery = "") => {
        const q = searchQuery.trim().toLowerCase();
        const projects = otherProjects.filter(
          (p) => !q || `${p.name} ${p.slug}`.toLowerCase().includes(q),
        );
        return {
          projects,
          isLoading: false,
          isLoadingMore: false,
          hasMore: false,
          loadMore: () => {},
          debouncedQuery: searchQuery,
          pageSize: 20,
        };
      },
    );
    mocked(useInfiniteDeployments).mockImplementation(
      (_teamId, searchQuery = "") => {
        const q = searchQuery.trim().toLowerCase();
        const deployments = [devDeployment].filter(
          (d) =>
            !q ||
            `${"reference" in d ? d.reference : ""} ${d.name}`
              .toLowerCase()
              .includes(q),
        );
        return {
          deployments,
          isLoading: false,
          isLoadingMore: false,
          hasMore: false,
          loadMore: () => {},
          debouncedQuery: searchQuery,
          pageSize: 25,
        };
      },
    );
    mocked(useDeployments).mockReturnValue({
      deployments: [devDeployment],
      isLoading: false,
    });
  },
} satisfies Meta<typeof ProjectSelector>;

export default meta;
type Story = StoryObj<typeof meta>;

// Inside a project: the selector shows the team avatar and the project name as
// two separate segments. Click the avatar to switch team, or the name to
// switch project.
export const TeamAndProject: Story = {
  args: {
    teams: [mockTeam],
    selectedTeamSlug: mockTeam.slug,
    selectedProject: mockProject,
  },
};

// At the team level (no project selected): a single button that opens the team
// switcher.
export const TeamOnly: Story = {
  args: {
    teams: [mockTeam],
    selectedTeamSlug: mockTeam.slug,
    selectedProject: undefined,
  },
};

// Opens the anchored team switcher, the menu the selector's avatar drops down.
async function openTeamMenu() {
  await userEvent.click(await screen.findByLabelText("Switch team"));
}

// The team switcher's "Available Teams" section: a dashed "Join Example
// Org" entry above the teams the member already belongs to.
export const TeamMenuWithJoinOffer: Story = {
  args: TeamOnly.args,
  beforeEach: () => {
    mocked(useDirectorySyncOffers).mockReturnValue([directorySyncOffer]);
  },
  play: openTeamMenu,
};

// Selecting the join entry closes the menu and prompts, naming the email that
// makes the caller eligible, with the choice to join or ignore.
export const JoinOfferConfirmation: Story = {
  ...TeamMenuWithJoinOffer,
  play: async () => {
    await openTeamMenu();
    await userEvent.click(
      await screen.findByText(`Join ${directorySyncOffer.teamName}`),
    );
  },
};

// Ignoring the offer drops it from the team switcher and points at the Profile
// page, which keeps listing every team the member can join.
export const JoinOfferIgnored: Story = {
  ...TeamMenuWithJoinOffer,
  play: async (context) => {
    await JoinOfferConfirmation.play!(context);
    await userEvent.click(
      await screen.findByRole("button", { name: "Ignore" }),
    );
  },
};
