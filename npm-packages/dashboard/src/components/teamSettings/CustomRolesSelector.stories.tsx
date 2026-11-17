import { Meta, StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import { useState } from "react";
import { userEvent, within } from "storybook/test";
import type { CustomRoleResponse } from "@convex-dev/platform/managementApi";
import { CustomRolesSelector } from "./CustomRolesSelector";

const AREAS = ["Payments", "Search", "Billing", "Infra", "Data Platform"];
const ACCESS = ["Read Only", "Deployer", "Log Viewer", "Env Var Editor"];

function makeRoles(areas: string[], access: string[]): CustomRoleResponse[] {
  return areas.flatMap((area, i) =>
    access.map((level, j) => ({
      id: i * access.length + j + 1,
      teamId: 1,
      name: `${area} ${level}`,
      statements: [],
      createTime: 0,
    })),
  );
}

const roles = makeRoles(AREAS, ACCESS);

const meta = {
  component: CustomRolesSelector,
  args: {
    availableRoles: roles,
    selectedIds: [1, 6],
    onChange: () => {},
  },
  render: function Render(args) {
    const [ids, setIds] = useState(args.selectedIds);
    return (
      <Sheet className="max-w-sm">
        <CustomRolesSelector {...args} selectedIds={ids} onChange={setIds} />
      </Sheet>
    );
  },
} satisfies Meta<typeof CustomRolesSelector>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Empty: Story = {
  args: { selectedIds: [] },
};

export const Searching: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("combobox"), "deployer");
  },
};

export const NoMatches: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("combobox"), "marketing");
  },
};

// A team with 150 roles, and a selection of 25 of them.
export const ManyRoles: Story = {
  args: {
    availableRoles: makeRoles(
      [...AREAS, "Mobile", "Growth", "Support", "Security", "ML"],
      [
        ...ACCESS,
        "Operator",
        "Prod Access",
        "Staging Access",
        "Backup Admin",
        "Integrations",
        "Auditor",
        "Oncall",
        "Contractor",
        "Intern",
        "Lead",
        "Release Manager",
      ],
    ),
    selectedIds: Array.from({ length: 25 }, (_, i) => ((i * 7) % 150) + 1),
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("combobox"), "payments");
  },
};

export const NoRolesExist: Story = {
  args: { availableRoles: [], selectedIds: [] },
};
