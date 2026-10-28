import { Meta, StoryObj } from "@storybook/nextjs";
import { AuditLogEventResponse } from "@convex-dev/platform/managementApi";
import { MemberResponse, TeamResponse } from "generatedApi";
import { AuditLogAction } from "api/auditLog";
import { AuditLogItem } from "./AuditLogItem";
import { Sheet } from "@ui/Sheet";

const team: TeamResponse = {
  id: 1,
  slug: "team-slug",
  name: "Team Name",
  creator: 1,
  suspended: false,
  referralCode: "TEAM123",

  referredBy: null,
};

const member: MemberResponse = {
  id: 1,
  name: "John Doe",
  email: "member@convex.dev",
};

const meta = {
  component: AuditLogItem,
  args: {
    team,
    memberId: 1,
    members: [member],
  },
  parameters: { a11y: { test: "todo" } },
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
} satisfies Meta<typeof AuditLogItem>;

export default meta;
type Story = StoryObj<typeof meta>;

export const SpendingLimitChange: Story = {
  args: {
    entry: {
      id: 1,
      createTime: new Date().toISOString(),
      action: "billing:spendingLimit:update" as AuditLogAction,
      actor: { kind: "member", member_id: 1 },
      metadata: {
        previous: {
          warningThresholdCents: 500_00,
          disableThresholdCents: 1000_00,
          state: "Running",
        },
        current: {
          warningThresholdCents: 5000_00,
          disableThresholdCents: 10000_00,
          state: "Running",
        },
      },
    } as unknown as AuditLogEventResponse,
  },
};

export const SpendingLimitChangeAddAndRemove: Story = {
  args: {
    entry: {
      id: 1,
      createTime: new Date().toISOString(),
      action: "billing:spendingLimit:update" as AuditLogAction,
      actor: { kind: "member", member_id: 1 },
      metadata: {
        previous: {
          disableThresholdCents: null,
          warningThresholdCents: 0,
          state: "Running",
        },
        current: {
          disableThresholdCents: 3200_00,
          warningThresholdCents: null,
          state: "Running",
        },
      },
    } as unknown as AuditLogEventResponse,
  },
};

export const SpendingLimitChangeOnlyOneValue: Story = {
  args: {
    entry: {
      id: 1,
      createTime: new Date().toISOString(),
      action: "billing:spendingLimit:update" as AuditLogAction,
      actor: { kind: "member", member_id: 1 },
      metadata: {
        previous: {
          disableThresholdCents: 12345_00,
          warningThresholdCents: null,
          state: "Running",
        },
        current: {
          disableThresholdCents: 54321_00,
          warningThresholdCents: null,
          state: "Running",
        },
      },
    } as unknown as AuditLogEventResponse,
  },
};
