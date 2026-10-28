import { Meta, StoryObj } from "@storybook/nextjs";
import { TeamResponse } from "generatedApi";
import {
  InvitationResponse,
  TeamMember,
} from "@convex-dev/platform/managementApi";

import { TeamMemberList } from "./TeamMemberList";

const meta = {
  component: TeamMemberList,
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
  },
};

export const LoadingState: Story = {
  args: {
    members: undefined,
    invites: undefined,
    team,
  },
};
