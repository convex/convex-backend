import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, fn } from "storybook/test";
import { useTeamAuditLog } from "api/auditLog";
import { AuditLogPage } from "../../pages/t/[team]/settings/audit-log";

// Fixed "now" so the date-range picker and relative timestamps are stable.
const NOW = new Date("2026-03-10T14:25:00Z").getTime();

const meta = {
  component: AuditLogPage,
  parameters: {
    layout: "fullscreen",
    a11y: {
      test: "todo",
    },
  },
  beforeEach: () => {
    const originalDateNow = Date.now;
    Date.now = () => NOW;

    mocked(useTeamAuditLog).mockReturnValue({
      entries: [
        {
          action: "team:join",
          actor: { kind: "member", member_id: 2 },
          createTime: new Date("2026-03-10T14:23:00Z").getTime(),
          metadata: {},
          teamId: 2,
        },
        {
          action: "member:invite",
          actor: { kind: "member", member_id: 1 },
          createTime: new Date("2026-03-10T14:22:00Z").getTime(),
          metadata: { noun: "member", current: { email: "pepper@convex.dev" } },
          teamId: 2,
        },
        {
          action: "project:create",
          actor: { kind: "member", member_id: 1 },
          createTime: new Date("2026-03-10T14:21:00Z").getTime(),
          metadata: {
            noun: "project",
            current: { id: 7, name: "Amazing SaaS Product" },
          },
          teamId: 2,
        },
        {
          action: "deployment:create",
          actor: { kind: "member", member_id: 1 },
          createTime: new Date("2026-03-10T14:20:00Z").getTime(),
          metadata: {
            current: { deploymentType: "dev", projectId: 7 },
          },
          teamId: 2,
        },
        {
          action: "billing:subscription:create",
          actor: { kind: "member", member_id: 1 },
          createTime: new Date("2026-03-10T14:19:00Z").getTime(),
          metadata: { current: { plan: "Convex Professional" } },
          teamId: 2,
        },
        {
          action: "team:create",
          actor: { kind: "member", member_id: 1 },
          createTime: new Date("2026-03-10T14:18:00Z").getTime(),
          metadata: {},
          teamId: 2,
        },
      ],
      isLoading: false,
      loadNextPage: fn(),
      hasMore: false,
    });

    return () => {
      Date.now = originalDateNow;
    };
  },
} satisfies Meta<typeof AuditLogPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
