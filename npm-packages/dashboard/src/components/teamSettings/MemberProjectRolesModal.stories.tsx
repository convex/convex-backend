import { Meta, StoryObj } from "@storybook/nextjs";
import { fn, mocked } from "storybook/test";
import type { TeamMember } from "@convex-dev/platform/managementApi";
import type { ProjectDetails, TeamResponse } from "generatedApi";
import { usePaginatedProjects } from "api/projects";
import { useHasProjectAdminPermissions } from "api/roles";
import { MemberProjectRolesModal } from "./MemberProjectRolesModal";

const team = { id: 1, name: "Acme", slug: "acme" } as TeamResponse;

const member = {
  id: 2,
  name: "Alan Turing",
  email: "alan@example.com",
  role: "developer",
} as TeamMember;

// Slugs usually match the name; a few differ to show the secondary line.
const projectNames: [name: string, slug: string][] = [
  ["Analytics Pipeline", "analytics-pipeline"],
  ["billing-service", "billing-service"],
  ["chat-app", "chat-app"],
  ["Docs Site", "docs-site"],
  ["internal-tools", "internal-tools"],
  ["marketing-site", "marketing-site"],
  ["Mobile Backend", "mobile-backend"],
  ["my-amazing-app", "my-amazing-app"],
  ["search-indexer", "search-indexer"],
  ["status-page", "status-page"],
];

const projects = projectNames.map(
  ([name, slug], i) =>
    ({
      id: i + 1,
      name,
      slug,
      teamId: team.id,
    }) as ProjectDetails,
);

// Smaller than the default page size so the story has a second page.
const STORY_PAGE_SIZE = 6;

const meta = {
  component: MemberProjectRolesModal,
  args: {
    team,
    member,
    projectRoles: [
      { memberId: 2, projectId: 3, teamId: 1, role: "admin", lastUpdated: 0 },
      { memberId: 2, projectId: 8, teamId: 1, role: "admin", lastUpdated: 0 },
    ],
    onUpdateProjectRoles: fn(),
    onClose: fn(),
  },
  beforeEach: () => {
    // Filters by `q` and pages by `cursor` like the real endpoint, so search
    // and pagination can be exercised. The cursor is the next page's offset.
    mocked(usePaginatedProjects).mockImplementation(
      (_teamId, { q, cursor }) => {
        const matches = q
          ? projects.filter((p) =>
              p.name.toLowerCase().includes(q.toLowerCase()),
            )
          : projects;
        const start = cursor ? Number(cursor) : 0;
        const end = start + STORY_PAGE_SIZE;
        const hasMore = end < matches.length;
        return {
          items: matches.slice(start, end),
          pagination: {
            hasMore,
            nextCursor: hasMore ? String(end) : undefined,
          },
          isLoading: false,
        } as ReturnType<typeof usePaginatedProjects>;
      },
    );
    mocked(useHasProjectAdminPermissions).mockReturnValue(true);
  },
} satisfies Meta<typeof MemberProjectRolesModal>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithoutPermission: Story = {
  beforeEach: () => {
    mocked(useHasProjectAdminPermissions).mockReturnValue(false);
  },
};

export const SomeProjectsWithoutPermission: Story = {
  beforeEach: () => {
    mocked(useHasProjectAdminPermissions).mockImplementation(
      (projectId) => projectId === undefined || projectId % 3 !== 0,
    );
  },
};

export const NoProjects: Story = {
  beforeEach: () => {
    mocked(usePaginatedProjects).mockReturnValue({
      items: [],
      pagination: { hasMore: false },
      isLoading: false,
    } as ReturnType<typeof usePaginatedProjects>);
  },
};
