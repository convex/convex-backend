import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, fn } from "storybook/test";
import { InvitationResponse } from "@convex-dev/platform/managementApi";
import { useRemoveTeamMember } from "api/teams";
import {
  useTeamInvites,
  useCreateInvite,
  useCancelInvite,
} from "api/invitations";
import {
  useIsCurrentMemberTeamAdmin,
  useProjectRoles,
  useUpdateTeamMemberRole,
  useUpdateProjectRoles,
} from "api/roles";
import { TeamMembersPage } from "../../pages/t/[team]/settings/members";

const mockInvites: InvitationResponse[] = [];

const mockProjectRoles: ReturnType<typeof useProjectRoles>["projectRoles"] = [];

const meta = {
  component: TeamMembersPage,
  parameters: {
    layout: "fullscreen",
    a11y: {
      test: "todo",
    },
  },
  beforeEach: () => {
    mocked(useRemoveTeamMember).mockReturnValue(fn());
    mocked(useTeamInvites).mockReturnValue(mockInvites);
    mocked(useCreateInvite).mockReturnValue(fn());
    mocked(useCancelInvite).mockReturnValue(fn());
    mocked(useIsCurrentMemberTeamAdmin).mockReturnValue(true);
    mocked(useProjectRoles).mockReturnValue({
      isLoading: false,
      projectRoles: mockProjectRoles,
    });
    mocked(useUpdateTeamMemberRole).mockReturnValue(fn());
    mocked(useUpdateProjectRoles).mockReturnValue(fn());
  },
} satisfies Meta<typeof TeamMembersPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
