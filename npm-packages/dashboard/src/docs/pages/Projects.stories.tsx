import { Meta, StoryObj } from "@storybook/nextjs";
import { useState } from "react";
import { mocked, screen, userEvent } from "storybook/test";
import {
  PlatformDeploymentResponse,
  ProjectResponse,
} from "@convex-dev/platform/managementApi";
import { useDirectorySyncOffers } from "api/directorySync";
import { useTeams } from "api/teams";
import { useDeploymentByName } from "api/deployments";
import {
  usePaginatedProjects,
  useProjectById,
  useTeamProjectTags,
} from "api/projects";
import { useHasCustomRolePermission } from "api/roles";
import { useProjectTagFilter } from "hooks/useProjectTagFilter";
import { CreateProjectModal } from "hooks/useCreateProjectModal";
import { TeamIndexPage } from "../../pages/t/[team]";

const meta = {
  component: TeamIndexPage,
  parameters: {
    layout: "fullscreen",
    a11y: {
      test: "todo",
    },
  },
} satisfies Meta<typeof TeamIndexPage>;

export default meta;
type Story = StoryObj<typeof meta>;

const projectDefaults = {
  teamId: 2,
  teamSlug: "acme",
  prodDeploymentReference: "production",
  devDeploymentReference: "dev/nicolas",
};

const projects: ProjectResponse[] = [
  {
    ...projectDefaults,
    id: 7,
    name: "My amazing app",
    slug: "my-amazing-app",
    createTime: Date.now() - 2 * 24 * 60 * 60 * 1000,
    prodDeploymentName: "musical-otter-456",
    devDeploymentName: "happy-capybara-123",
    tags: ["frontend", "billing"],
  },
  {
    ...projectDefaults,
    id: 8,
    name: "Analytics pipeline",
    slug: "analytics-pipeline",
    createTime: Date.now() - 60 * 24 * 60 * 60 * 1000,
    prodDeploymentName: "brave-falcon-219",
    devDeploymentName: "gentle-lemur-874",
    tags: ["data", "internal"],
  },
  {
    ...projectDefaults,
    id: 9,
    name: "Payments worker",
    slug: "payments-worker",
    createTime: Date.now() - 120 * 24 * 60 * 60 * 1000,
    prodDeploymentName: "steady-moose-602",
    devDeploymentName: "clever-badger-331",
    tags: ["backend", "billing"],
  },
  {
    ...projectDefaults,
    id: 10,
    name: "Marketing site",
    slug: "marketing-site",
    createTime: Date.now() - 365 * 24 * 60 * 60 * 1000,
    prodDeploymentName: "sunny-otter-145",
    devDeploymentName: null,
    devDeploymentReference: null,
    tags: ["frontend"],
  },
];

// The deployment the member most recently opened in "My amazing app", which
// its card suggests after the project's default deployments.
const recentDeployment: PlatformDeploymentResponse = {
  kind: "cloud",
  id: 321,
  name: "quiet-heron-321",
  createTime: Date.now(),
  deploymentType: "preview",
  projectId: 7,
  region: "aws-us-east-1",
  isDefault: false,
  reference: "preview/fix-login-redirect",
  previewIdentifier: "fix-login-redirect",
  deploymentUrl: "https://quiet-heron-321.convex.cloud",
  class: "s16",
};

export const Default: Story = {
  beforeEach: () => {
    const key = `/lastViewedNonDefaultDeploymentForProject/${recentDeployment.projectId}`;
    window.localStorage.setItem(
      key,
      JSON.stringify({
        name: recentDeployment.name,
        reference: recentDeployment.reference,
        deploymentType: recentDeployment.deploymentType,
      }),
    );
    return () => window.localStorage.removeItem(key);
  },
  decorators: [
    (storyFn) => {
      mocked(usePaginatedProjects).mockReturnValue({
        items: projects,
        pagination: { hasMore: false },
        isLoading: false,
      });
      mocked(useProjectById).mockImplementation((projectId) => ({
        project: projects.find((p) => p.id === projectId),
        isLoading: false,
        error: undefined,
      }));
      mocked(useTeamProjectTags).mockReturnValue([
        ...new Set(projects.flatMap((p) => p.tags)),
      ]);
      mocked(useDeploymentByName).mockImplementation((deploymentName) => ({
        deployment:
          deploymentName === recentDeployment.name
            ? recentDeployment
            : undefined,
        notFound: false,
      }));
      return storyFn();
    },
  ],
};

// The team the docs decorator mocks, plus enough others for the switcher to
// look like a list rather than a single row.
const teams = [
  {
    id: 2,
    creator: 1,
    slug: "acme",
    name: "Acme Corp",
    suspended: false,
    referralCode: "ACME01",
    referredBy: null,
  },
  {
    id: 3,
    creator: 1,
    slug: "acme-labs",
    name: "Acme Labs",
    suspended: false,
    referralCode: "ACME02",
    referredBy: null,
  },
  {
    id: 4,
    creator: 1,
    slug: "riley",
    name: "Riley’s Team",
    suspended: false,
    referralCode: "RILEY01",
    referredBy: null,
  },
];

/**
 * The team switcher the header's team name opens.
 */
export const TeamSwitcher: Story = {
  parameters: {
    screenshotSelector:
      '[aria-label="Switch team"], .command-palette--anchored',
    // The menu's list is capped at min(330px, 40vh): at the default 700px-tall
    // viewport the 40vh half of that clips the last team.
    screenshotViewport: { width: 1024, height: 1000 },
  },
  decorators: [
    (storyFn) => {
      mocked(useTeams).mockReturnValue({
        selectedTeamSlug: "acme",
        teams,
      });
      return storyFn();
    },
  ],
  play: async () => {
    // The header is rendered by the docs decorator and the palette portals to
    // document.body, so query the whole screen rather than the story canvas.
    await userEvent.click(await screen.findByLabelText("Switch team"));
    await screen.findByText("Switch Team");
  },
};

/**
 * How a team a directory provisions you into reaches you: as an invitation in
 * the team switcher, alongside the teams you already belong to.
 */
export const TeamSwitcherWithInvitation: Story = {
  ...TeamSwitcher,
  decorators: [
    (storyFn) => {
      mocked(useTeams).mockReturnValue({
        selectedTeamSlug: "acme-labs",
        // Only the teams they're already in; Acme Corp is the one on offer.
        teams: teams.filter((team) => team.slug !== "acme"),
      });
      mocked(useDirectorySyncOffers).mockReturnValue([
        { teamId: 2, teamName: "Acme Corp", email: "dana@acme.dev" },
      ]);
      return storyFn();
    },
  ],
};

// Stands in for `useProjectTagFilter`, whose URL updates the Storybook router
// ignores, so picking tags in a story changes the selection.
function fakeTagFilter(initialTags: string[]) {
  return function useFakeTagFilter(): ReturnType<typeof useProjectTagFilter> {
    return useState(initialTags);
  };
}

const taggedProject = {
  teamId: 2,
  teamSlug: "acme",
  createTime: new Date("2026-04-01T09:41:00Z").getTime(),
  prodDeploymentName: "musical-otter-456",
  devDeploymentName: "happy-capybara-123",
};

/**
 * The project list filtered to the projects with one tag.
 */
export const FilteredByTag: Story = {
  decorators: [
    (storyFn) => {
      mocked(useProjectTagFilter).mockImplementation(
        fakeTagFilter(["billing"]),
      );
      mocked(useTeamProjectTags).mockReturnValue([
        "backend",
        "billing",
        "production",
        "team-payments",
      ]);
      mocked(usePaginatedProjects).mockReturnValue({
        items: [
          {
            ...taggedProject,
            id: 8,
            name: "Invoicing",
            slug: "invoicing",
            tags: ["billing"],
          },
          {
            ...taggedProject,
            id: 9,
            name: "Payments worker",
            slug: "payments-worker",
            tags: ["backend", "billing", "team-payments"],
          },
        ],
        pagination: { hasMore: false },
        isLoading: false,
      });
      return storyFn();
    },
  ],
};

/**
 * A custom-role member without `team:projectTags:view` gets the tag filter
 * disabled.
 */
export const TagFilterWithoutPermission: Story = {
  decorators: [
    (storyFn) => {
      mocked(useHasCustomRolePermission).mockImplementation(
        (_teamId, action) => action !== "team:projectTags:view",
      );
      return storyFn();
    },
  ],
};

/**
 * The tag filter is disabled while none of the team's projects have tags.
 */
export const TagFilterWithoutTags: Story = {
  decorators: [
    (storyFn) => {
      mocked(useTeamProjectTags).mockReturnValue([]);
      mocked(usePaginatedProjects).mockReturnValue({
        items: [{ ...projects[0], tags: [] }],
        pagination: { hasMore: false },
        isLoading: false,
      });
      return storyFn();
    },
  ],
};

/**
 * The Create Project dialog with tags added to the new project.
 */
export const CreateProjectWithTags: Story = {
  parameters: {
    screenshotSelector: '[role="dialog"]',
  },
  // The dashboard mounts this dialog once in `_app`, outside the page.
  render: (args) => (
    <>
      <TeamIndexPage {...args} />
      <CreateProjectModal />
    </>
  ),
  play: async () => {
    await userEvent.click(
      await screen.findByRole("button", { name: "Create Project" }),
    );
    await userEvent.type(await screen.findByLabelText("Project Name"), "Shop");
    await userEvent.type(
      screen.getByRole("combobox", { name: "Tags" }),
      "frontend{Enter}billing{Enter}",
    );
  },
};
