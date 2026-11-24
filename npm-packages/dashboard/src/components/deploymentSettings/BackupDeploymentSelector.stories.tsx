import { Meta, StoryObj } from "@storybook/nextjs";
import {
  expect,
  fireEvent,
  fn,
  mocked,
  userEvent,
  waitFor,
  within,
} from "storybook/test";
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
    tags: [],
  },
  {
    id: 2,
    name: "Project Beta",
    slug: "project-beta",
    teamId: 1,
    teamSlug: "test-team",
    createTime: Date.now() - 60 * 24 * 60 * 60 * 1000,
    tags: [],
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
      pages: [mockProjects],
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
      tags: [],
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
          pages: data,
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

// --- Menu placement ----------------------------------------------------------
// These stories check where the anchored menu lands relative to its trigger
// and the viewport. Rects are compared with a pixel of slack for subpixel
// rounding.

const SLACK = 1;

// The geometry the menu is meant to have, stated here rather than imported so
// a change to the palette's margins or minimum height has to update these too.
const BOTTOM_MARGIN = 64;
const TOP_MARGIN = 16;
const MIN_HEIGHT = 320;

// The palette's open state is global and outlives a story, and while it's open
// the rest of the page is hidden from the accessibility tree, so each story
// starts and ends with the palette closed. A press outside closes it in one
// step, whichever page it's on; it's dispatched rather than clicked because
// the open dialog makes the page `pointer-events: none`, and the palette's
// click-away listener reacts to `mousedown`.
async function closeMenu() {
  if (!document.querySelector('[aria-haspopup="menu"][aria-expanded="true"]')) {
    return;
  }
  await fireEvent.mouseDown(document.body);
  await waitFor(() => {
    if (document.querySelector("[cmdk-root]")) {
      throw new Error("The menu is still open");
    }
  });
}

// Opens the menu from the trigger and resolves with its surface once Floating
// UI has positioned it and the list's height transition has settled.
async function openMenu(canvasElement: HTMLElement) {
  await closeMenu();
  await userEvent.click(
    within(canvasElement).getByRole("button", { name: /Restore from/ }),
  );
  const menu = await waitFor(() => {
    const element = document.querySelector<HTMLElement>(
      "[cmdk-root].command-palette--anchored",
    );
    if (!element || getComputedStyle(element).opacity === "0") {
      throw new Error("The menu isn't positioned yet");
    }
    return element;
  });
  await waitForSettledMenu(menu);
  return menu;
}

async function waitForSettledMenu(element: HTMLElement) {
  let previous = { top: NaN, height: NaN };
  await waitFor(
    () => {
      const { top, height } = element.getBoundingClientRect();
      const stable = top === previous.top && height === previous.height;
      previous = { top, height };
      if (!stable) {
        throw new Error("The menu is still moving or resizing");
      }
    },
    { interval: 150 },
  );
}

function placementStory(
  base: Story,
  play: (canvasElement: HTMLElement) => Promise<void>,
): Story {
  return {
    ...base,
    play: async ({ canvasElement }) => {
      try {
        await play(canvasElement);
      } finally {
        await closeMenu();
      }
    },
  };
}

// Steps back from the deployment list to the (long) project list and waits
// for it to render and the menu to finish growing.
async function showProjectList(menu: HTMLElement) {
  await userEvent.click(
    within(menu).getByRole("button", { name: "Select Project" }),
  );
  await within(menu).findByRole("option", { name: /Analytics Dashboard 1/ });
  await waitForSettledMenu(menu);
}

function listOf(menu: HTMLElement) {
  const list = menu.querySelector<HTMLElement>("[cmdk-list]");
  if (!list) {
    throw new Error("The menu has no list");
  }
  return list;
}

// A list longer than the viewport grows until it stops the bottom margin short
// of the viewport edge, then scrolls.
export const LongListFillsToBottomMargin = placementStory(
  ManyProjects,
  async (canvasElement) => {
    const menu = await openMenu(canvasElement);
    await showProjectList(menu);

    // Retried: cmdk measures the list after it renders and then animates the
    // height, so the menu can still be growing when it first looks settled.
    await waitFor(async () => {
      const gap = window.innerHeight - menu.getBoundingClientRect().bottom;
      await expect(gap).toBeGreaterThanOrEqual(BOTTOM_MARGIN - SLACK);
      await expect(gap).toBeLessThanOrEqual(BOTTOM_MARGIN + SLACK);
    });
    const list = listOf(menu);
    await expect(list.scrollHeight).toBeGreaterThan(list.clientHeight);
  },
);

// A list that fits ends where its items do instead of filling the room below
// the trigger, so it's clear there's nothing more to scroll to.
export const ShortListFitsContent = placementStory(
  Default,
  async (canvasElement) => {
    const menu = await openMenu(canvasElement);
    // The menu is hidden until it's positioned, which mustn't keep the search
    // input from taking focus when it opens.
    await expect(within(menu).getByRole("combobox")).toHaveFocus();
    const list = listOf(menu);
    await expect(list.scrollHeight).toBeLessThanOrEqual(list.clientHeight);
    await expect(menu.getBoundingClientRect().bottom).toBeLessThan(
      window.innerHeight - BOTTOM_MARGIN - 100,
    );
  },
);

// With little room under the trigger, the menu opens above it, stays inside
// the viewport, and keeps its footer whole.
export const OpensAboveTriggerNearBottom = placementStory(
  {
    ...ManyProjects,
    render: (args) => (
      <div className="flex h-screen flex-col justify-end pb-8">
        <BackupDeploymentSelectorHarness {...args} />
      </div>
    ),
  },
  async (canvasElement) => {
    const menu = await openMenu(canvasElement);
    await showProjectList(menu);

    // `hidden`: the open palette hides the rest of the page from role queries.
    // Measured now rather than before opening, since the page can still be
    // settling when the story first renders.
    const trigger = within(canvasElement)
      .getByRole("button", { name: /Restore from/, hidden: true })
      .getBoundingClientRect();
    const rect = menu.getBoundingClientRect();
    await expect(rect.bottom).toBeLessThanOrEqual(trigger.top + SLACK);
    await expect(rect.top).toBeGreaterThanOrEqual(TOP_MARGIN - SLACK);

    const footer = menu.lastElementChild!.getBoundingClientRect();
    await expect(footer.height).toBeGreaterThan(0);
    await expect(footer.bottom).toBeLessThanOrEqual(rect.bottom + SLACK);
  },
);

// In a window too short for the menu on either side of the trigger, it opens
// centered like the ⌘K palette, keeps its minimum height, and only its list
// shrinks: the input and footer stay whole and inside the window.
export const CenteredInShortWindow = placementStory(
  {
    ...ManyProjects,
    parameters: {
      ...ManyProjects.parameters,
      viewport: {
        options: {
          shortWindow: {
            name: "Short window",
            styles: { width: "800px", height: "340px" },
          },
        },
      },
    },
    globals: { viewport: { value: "shortWindow" } },
  },
  async (canvasElement) => {
    const menu = await openMenu(canvasElement);
    await showProjectList(menu);

    const rect = menu.getBoundingClientRect();
    await expect(rect.top).toBeGreaterThanOrEqual(0);
    await expect(rect.bottom).toBeLessThanOrEqual(window.innerHeight + SLACK);
    await expect(
      Math.abs((rect.left + rect.right) / 2 - window.innerWidth / 2),
    ).toBeLessThanOrEqual(SLACK);
    await expect(rect.height).toBeGreaterThanOrEqual(
      Math.min(MIN_HEIGHT, window.innerHeight - 32) - SLACK,
    );

    const input = within(menu).getByRole("combobox").getBoundingClientRect();
    await expect(input.top).toBeGreaterThanOrEqual(rect.top - SLACK);
    const footer = menu.lastElementChild!.getBoundingClientRect();
    await expect(footer.height).toBeGreaterThan(0);
    await expect(footer.bottom).toBeLessThanOrEqual(rect.bottom + SLACK);
    const list = listOf(menu);
    await expect(list.scrollHeight).toBeGreaterThan(list.clientHeight);
  },
);
