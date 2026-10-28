import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, fn } from "storybook/test";
import React from "react";
import { TeamResponse } from "generatedApi";
import { useProfile } from "api/profile";
import {
  useCurrentProject,
  useInfiniteProjects,
  useProjectById,
} from "api/projects";
import { useDeployments } from "api/deployments";
import { useCurrentTeam, useTeamMembers } from "api/teams";
import { CommandPalette } from "elements/CommandPalette";
import {
  PlatformDeploymentResponse,
  ProjectResponse,
  TeamMember,
} from "@convex-dev/platform/managementApi";
import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { BackupDeploymentSelector } from "./BackupDeploymentSelector";

const team: TeamResponse = {
  id: 1,
  creator: 1,
  slug: "test-team",
  name: "Test Team",
  suspended: false,
  referralCode: "TEAM123",
  referredBy: null,
};

const mockProjects: ProjectResponse[] = [
  {
    id: 1,
    name: "Project Alpha",
    slug: "project-alpha",
    teamId: 1,
    teamSlug: "test-team",
    createTime: Date.now() - 30 * 24 * 60 * 60 * 1000,
  },
  {
    id: 2,
    name: "Project Beta",
    slug: "project-beta",
    teamId: 1,
    teamSlug: "test-team",
    createTime: Date.now() - 60 * 24 * 60 * 60 * 1000,
  },
];

const mockTeamMembers: TeamMember[] = [
  {
    id: 1,
    email: "test@example.com",
    name: "Test User",
    role: "admin",
    customRoles: [],
  },
  {
    id: 2,
    email: "jane@example.com",
    name: "Jane Doe",
    role: "developer",
    customRoles: [],
  },
  {
    id: 3,
    email: "bob@example.com",
    name: "Bob Smith",
    role: "developer",
    customRoles: [],
  },
];

const createDeployment = (overrides: {
  id: number;
  name: string;
  deploymentType: "prod" | "dev" | "preview" | "custom";
  creator?: number | null;
  isDefault?: boolean;
  previewIdentifier?: string | null;
  reference: string;
}): PlatformDeploymentResponse => ({
  kind: "cloud",
  class: "s16",
  projectId: 1,
  creator: 1,
  createTime: Date.now() - 7 * 24 * 60 * 60 * 1000,
  region: "aws-us-east-1",
  isDefault: false,
  previewIdentifier: null,
  deploymentUrl: `https://${overrides.name}.convex.cloud`,
  ...overrides,
});

const mockDeployments: PlatformDeploymentResponse[] = [
  createDeployment({
    id: 1,
    name: "joyful-capybara-123",
    deploymentType: "prod",
    isDefault: true,
    creator: 1,
    reference: "production",
  }),
  createDeployment({
    id: 2,
    name: "happy-zebra-456",
    deploymentType: "dev",
    creator: 1, // Current user's dev deployment
    reference: "dev/test-user",
  }),
  createDeployment({
    id: 3,
    name: "musical-bird-918",
    deploymentType: "preview",
    previewIdentifier: "feature/new-ui",
    creator: 1,
    reference: "preview/feature/new-ui",
  }),
  createDeployment({
    id: 4,
    name: "peaceful-cow-184",
    deploymentType: "custom",
    creator: 1,
    reference: "staging",
  }),
  createDeployment({
    id: 5,
    name: "active-cat-205",
    deploymentType: "dev",
    creator: 2,
    reference: "dev/jane-doe",
  }),
  createDeployment({
    id: 6,
    name: "friendly-dog-321",
    deploymentType: "dev",
    creator: 3,
    reference: "dev/bob-smith",
  }),
];

// The selector opens the palette anchored beneath its trigger, so a story
// needs the palette mounted alongside it. It's also controlled, so the story
// holds the choice the way the backup list does and the trigger updates when
// you pick something.
function BackupDeploymentSelectorHarness({
  selectedDeployment,
  onChange,
  ...props
}: React.ComponentProps<typeof BackupDeploymentSelector>) {
  const [selected, setSelected] = useState(selectedDeployment);
  useEffect(() => setSelected(selectedDeployment), [selectedDeployment]);
  return (
    <div className="border-b">
      <BackupDeploymentSelector
        {...props}
        selectedDeployment={selected}
        onChange={(deployment) => {
          setSelected(deployment);
          onChange(deployment);
        }}
      />
      <CommandPalette />
    </div>
  );
}

const mockProfile = {
  id: 1,
  name: "Test User",
  email: "test@example.com",
};

const meta = {
  component: BackupDeploymentSelector,
  parameters: {
    layout: "fullscreen",
    // The anchored menu is a focus-trapping Radix dialog over an otherwise
    // empty canvas, which trips the automated a11y checks meant for full pages.
    a11y: { test: "todo" },
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/settings/backups",
        route: "/t/[team]/[project]/[deploymentName]/settings/backups",
        asPath:
          "/t/test-team/project-alpha/joyful-capybara-123/settings/backups",
        query: {
          team: "test-team",
          project: "project-alpha",
          deploymentName: "joyful-capybara-123",
        },
      },
    },
  },
  render: (args) => <BackupDeploymentSelectorHarness {...args} />,
  beforeEach: () => {
    mocked(useProfile).mockReturnValue(mockProfile);
    mocked(useCurrentTeam).mockReturnValue(team);
    mocked(useCurrentProject).mockReturnValue(mockProjects[0]);
    mocked(useInfiniteProjects).mockReturnValue({
      projects: mockProjects,
      isLoading: false,
      isLoadingMore: false,
      hasMore: false,
      loadMore: fn(),
      debouncedQuery: "",
      pageSize: 25,
    });
    mocked(useDeployments).mockReturnValue({
      deployments: mockDeployments,
      isLoading: false,
    });
    mocked(useTeamMembers).mockReturnValue(mockTeamMembers);
    mocked(useProjectById).mockReturnValue({
      project: mockProjects[0],
      isLoading: false,
      error: undefined,
    });
  },
} satisfies Meta<typeof BackupDeploymentSelector>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    selectedDeployment: mockDeployments[0],
    targetDeployment: mockDeployments[0],
    onChange: fn(),
  },
};

export const DifferentDeploymentSelected: Story = {
  args: {
    selectedDeployment: mockDeployments[4], // Jane's dev deployment
    targetDeployment: mockDeployments[0],
    onChange: fn(),
  },
};

// Deployments outside the target's region can't be restored from in the
// dashboard, so the menu lists them disabled with an explanation.
export const OtherRegionDeployments: Story = {
  beforeEach: () => {
    mocked(useDeployments).mockReturnValue({
      deployments: mockDeployments.map((deployment, i) =>
        i === 0 ? deployment : { ...deployment, region: "aws-eu-west-1" },
      ),
      isLoading: false,
    });
  },
  args: {
    selectedDeployment: mockDeployments[0],
    targetDeployment: mockDeployments[0],
    onChange: fn(),
  },
};

// Generate many projects to test infinite scroll behavior
let lastId = 0;
const generateManyProjects = (
  count: number,
  startOffset: number = 0,
): ProjectResponse[] => {
  const projectNames = [
    "Analytics Dashboard",
    "User Authentication",
    "Payment Processing",
    "Inventory Management",
    "Customer Portal",
    "Admin Console",
    "Marketing Site",
    "Mobile Backend",
    "Email Service",
    "Notification System",
    "Search Engine",
    "Content Management",
    "Reporting Tool",
    "Data Pipeline",
    "API Gateway",
  ];

  return Array.from({ length: count }, (_, i) => {
    const index = startOffset + i;
    return {
      id: lastId++,
      name: `${projectNames[index % projectNames.length]} ${Math.floor(index / projectNames.length) + 1}`,
      slug: `project-${lastId}`,
      teamId: 1,
      teamSlug: "test-team",
      createTime: Date.now() - index * 24 * 60 * 60 * 1000,
    };
  });
};

export const ManyProjects: Story = {
  beforeEach: () => {
    mocked(useProfile).mockReturnValue(mockProfile);
    mocked(useCurrentTeam).mockReturnValue(team);
    mocked(useCurrentProject).mockReturnValue(mockProjects[0]);
    mocked(useDeployments).mockReturnValue({
      deployments: mockDeployments,
      isLoading: false,
    });
    mocked(useTeamMembers).mockReturnValue(mockTeamMembers);
    mocked(useProjectById).mockReturnValue({
      project: mockProjects[0],
      isLoading: false,
      error: undefined,
    });
  },
  decorators: [
    (Story) => {
      const PAGE_SIZE = 25;
      const TOTAL_AVAILABLE = 200;
      const LOAD_DELAY_MS = 500;

      const [data, setData] = useState<ProjectResponse[][]>(() => [
        [
          // Include the project with the default deployment as the first item.
          mockProjects[0],
          // Then load the first page of generated projects.
          ...generateManyProjects(PAGE_SIZE - 1, 0),
        ],
      ]);
      const loadedProjects = useMemo(() => data.flat(), [data]);

      const hasMore = loadedProjects.length < TOTAL_AVAILABLE;
      const isLoadingMore = useRef(false);

      const loadMore = useCallback(() => {
        if (!hasMore || isLoadingMore.current) {
          return;
        }
        isLoadingMore.current = true;
        setTimeout(() => {
          setData((prev) => [
            ...prev,
            generateManyProjects(PAGE_SIZE, loadedProjects.length - 1),
          ]);
          isLoadingMore.current = false;
        }, LOAD_DELAY_MS);
      }, [hasMore, isLoadingMore, loadedProjects.length]);

      // Update mock on every render with current state
      useEffect(() => {
        mocked(useInfiniteProjects).mockReturnValue({
          projects: loadedProjects,
          isLoading: false,
          isLoadingMore: isLoadingMore.current,
          hasMore,
          loadMore,
          debouncedQuery: "",
          pageSize: PAGE_SIZE,
        });
      });

      return <Story />;
    },
  ],
  args: {
    selectedDeployment: mockDeployments[0],
    targetDeployment: mockDeployments[0],
    onChange: fn(),
  },
};
