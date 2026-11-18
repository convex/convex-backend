import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked } from "storybook/test";
import { useProfile } from "api/profile";
import { useProjectRoles } from "api/roles";
import { TeamResponse } from "generatedApi";
import {
  InvitationResponse,
  TeamMember,
} from "@convex-dev/platform/managementApi";

import { TeamMemberList } from "./TeamMemberList";

const meta = {
  component: TeamMemberList,
  beforeEach: () => {
    // The member list renders once the viewer's profile and project roles load.
    mocked(useProfile).mockReturnValue({
      id: 1,
      email: "user1@example.org",
      name: "User 1",
    } as ReturnType<typeof useProfile>);
    mocked(useProjectRoles).mockReturnValue({
      isLoading: false,
      projectRoles: [],
    });
  },
} satisfies Meta<typeof TeamMemberList>;

export default meta;
type Story = StoryObj<typeof meta>;

const members: TeamMember[] = [
  {
    id: 1,
    email: "user1@example.org",
    name: "User 1",
    role: "admin",
    customRoles: [],
  },
  {
    id: 2,
    email: "user2@example.org",
    role: "developer",
    customRoles: [],
  },
];
const invites: InvitationResponse[] = [
  {
    email: "user3@example.org",
    expired: false,
    role: "developer",
  },
  {
    email: "user4@example.org",
    expired: true,
    role: "admin",
  },
];
const team: TeamResponse = {
  id: 1,
  creator: 1,
  slug: "team",
  name: "Team",
  suspended: false,
  referralCode: "TEAM123",
  referredBy: null,
};

export const Primary: Story = {
  args: {
    members,
    invites,
    team,
    hasSyncedDirectory: false,
  },
};

export const LoadingState: Story = {
  args: {
    members: undefined,
    invites: undefined,
    team,
    hasSyncedDirectory: false,
  },
};

export const DirectorySynced: Story = {
  args: {
    members: members.map((member) => ({ ...member, directoryManaged: true })),
    invites,
    team,
    hasSyncedDirectory: true,
  },
};
