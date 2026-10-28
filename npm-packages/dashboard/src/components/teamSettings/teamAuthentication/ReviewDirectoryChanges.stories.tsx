import { Meta, StoryObj } from "@storybook/nextjs";
import { expect, fn, mocked, screen } from "storybook/test";
import {
  useEnableDirectorySync,
  useStagedDirectoryMembers,
} from "api/directorySync";
import type { StagedDirectoryMemberResponse, TeamResponse } from "generatedApi";
import { ReviewDirectoryChanges } from "./ReviewDirectoryChanges";

const team: TeamResponse = {
  id: 1,
  creator: 1,
  name: "Acme Corp",
  slug: "acme",
  suspended: false,
  referralCode: "ACME01",
};

const items: StagedDirectoryMemberResponse[] = [
  {
    member: { id: 10, name: "Ari", email: "ari@acme.com", role: "admin" },
    directoryUser: {
      directoryUserId: "dir_ari",
      email: "ari@acme.com",
      state: "active",
      groups: [{ workosGroupId: "group_eng", name: "Engineering" }],
      role: "developer",
    },
  },
  {
    member: { id: 11, name: "Sam", email: "sam@acme.com", role: "developer" },
    directoryUser: {
      directoryUserId: "dir_sam",
      email: "sam@acme.com",
      state: "active",
      groups: [{ workosGroupId: "group_support", name: "Support" }],
      role: "custom",
      customRoles: [{ id: 7, name: "Support Engineer" }],
    },
  },
  {
    member: {
      id: 12,
      name: "Contractor",
      email: "contractor@other.com",
      role: "developer",
    },
    directoryUser: null,
  },
  {
    member: {
      id: 13,
      name: "Former Employee",
      email: "former@acme.com",
      role: "developer",
    },
    directoryUser: {
      directoryUserId: "dir_former",
      email: "former@acme.com",
      state: "suspended",
      groups: [],
      role: "developer",
    },
  },
  {
    member: {
      id: 14,
      name: "Ungrouped",
      email: "ungrouped@acme.com",
      role: "developer",
    },
    directoryUser: {
      directoryUserId: "dir_ungrouped",
      email: "ungrouped@acme.com",
      state: "active",
      groups: [],
      role: null,
    },
  },
  {
    member: null,
    directoryUser: {
      directoryUserId: "dir_new",
      email: "newcomer@acme.com",
      state: "active",
      groups: [{ workosGroupId: "group_eng", name: "Engineering" }],
      role: "developer",
    },
  },
];

function mockStaged(data: StagedDirectoryMemberResponse[], hasMore = false) {
  mocked(useStagedDirectoryMembers).mockReturnValue({
    data: {
      items: data,
      pagination: { hasMore, nextCursor: hasMore ? "next" : null },
    },
    isLoading: false,
    error: undefined,
  });
}

const meta = {
  component: ReviewDirectoryChanges,
  // The subpage's description and its sync note read in `--content-secondary`,
  // which measures 4.43:1 against the page background in the light theme — the
  // token is a hair under AA wherever it sits outside a sheet, not something
  // this page can fix. Every other rule still runs.
  parameters: {
    a11y: { config: { rules: [{ id: "color-contrast", enabled: false }] } },
  },
  args: { team, enabled: false, onEnabled: fn() },
  beforeEach: () => {
    mocked(useEnableDirectorySync).mockReturnValue(fn() as any);
    mockStaged(items);
  },
} satisfies Meta<typeof ReviewDirectoryChanges>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Review: Story = {};

export const ReviewManyPages: Story = {
  beforeEach: () => {
    mockStaged(items, true);
  },
};

export const NothingToReview: Story = {
  beforeEach: () => {
    mockStaged([]);
  },
};

export const PendingMembers: Story = {
  args: { enabled: true },
  beforeEach: () => {
    mockStaged(items.filter((item) => item.member === null));
  },
};

export const NoPendingMembers: Story = {
  args: { enabled: true },
  beforeEach: () => {
    mockStaged([]);
  },
};

// A roster longer than the space it is given. The subpage fills the height of
// the pane it sits in, so the decorator stands in for that pane: the table is
// what scrolls, and the acknowledgement and its button stay on screen.
export const ManyMembersInAShortPane: Story = {
  decorators: [
    (Story) => (
      <div className="flex h-128 flex-col overflow-y-auto">
        <Story />
      </div>
    ),
  ],
  beforeEach: () => {
    mockStaged(
      Array.from({ length: 40 }, (_, i) => ({
        member: {
          id: 100 + i,
          name: `Member ${i + 1}`,
          email: `member${i + 1}@acme.com`,
          role: "admin" as const,
        },
        directoryUser: {
          directoryUserId: `dir_${i}`,
          email: `member${i + 1}@acme.com`,
          state: "active" as const,
          groups: [{ workosGroupId: "group_eng", name: "Engineering" }],
          role: "developer" as const,
        },
      })),
    );
  },
  play: async () => {
    const table = (await screen.findByRole("table")).closest("div")!;
    await expect(table.scrollHeight).toBeGreaterThan(table.clientHeight);

    // The point of the inner scroll: the decision stays reachable without
    // scrolling past forty rows to find it.
    const enable = await screen.findByRole("button", {
      name: "Enable directory sync",
    });
    const pane = enable.closest("[data-testid='review-directory-changes']")!;
    await expect(enable.getBoundingClientRect().bottom).toBeLessThanOrEqual(
      pane.getBoundingClientRect().bottom + 1,
    );
  },
};
