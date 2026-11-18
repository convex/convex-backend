import { Meta, StoryObj } from "@storybook/nextjs";

import { mocked } from "storybook/test";
import { TeamMember } from "@convex-dev/platform/managementApi";
import { useHasCustomRolePermission, useListCustomRoles } from "api/roles";
import { useTeamEntitlements } from "api/teams";
import { InviteMemberForm } from "./InviteMemberForm";

const meta = { component: InviteMemberForm } satisfies Meta<
  typeof InviteMemberForm
>;

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
export const Primary: Story = {
  args: {
    team: {
      creator: 1,
      id: 1,
      name: "team",
      slug: "team",
      suspended: false,
      referralCode: "TEAM123",

      referredBy: null,
    },
    members,
    invites: [],
    canInvite: true,
  },
};

export const WithCustomRoles: Story = {
  args: Primary.args,
  beforeEach: () => {
    mocked(useHasCustomRolePermission).mockReturnValue(true);
    mocked(useTeamEntitlements).mockReturnValue({
      customRolesEnabled: true,
      maxTeamMembers: 100,
    } as ReturnType<typeof useTeamEntitlements>);
    mocked(useListCustomRoles).mockReturnValue({
      data: {
        items: ["Auditor", "Billing Admin", "Support Engineer"].map(
          (name, i) => ({
            id: i + 1,
            teamId: 1,
            name,
            statements: [],
            createTime: 0,
          }),
        ),
        pagination: { hasMore: false, nextCursor: null },
      },
    } as unknown as ReturnType<typeof useListCustomRoles>);
  },
};

export const OneSeatLeft: Story = {
  args: {
    ...Primary.args,
    invites: [
      { email: "pending@example.org", role: "developer", expired: false },
    ],
  },
  beforeEach: () => {
    mocked(useTeamEntitlements).mockReturnValue({
      customRolesEnabled: false,
      maxTeamMembers: 4,
    } as ReturnType<typeof useTeamEntitlements>);
  },
};
