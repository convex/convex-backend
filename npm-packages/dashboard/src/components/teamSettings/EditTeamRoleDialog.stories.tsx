import { Meta, StoryObj } from "@storybook/nextjs";
import type {
  CustomRoleResponse,
  TeamMember,
} from "@convex-dev/platform/managementApi";
import { TeamResponse } from "generatedApi";

import { EditTeamRoleDialog } from "./EditTeamRoleDialog";

const team: TeamResponse = {
  id: 1,
  creator: 1,
  name: "Convex team",
  slug: "convex-team",
  suspended: false,
  referralCode: "CODE123",
};

const meta = { component: EditTeamRoleDialog } satisfies Meta<
  typeof EditTeamRoleDialog
>;

export default meta;
type Story = StoryObj<typeof meta>;

const developer: TeamMember = {
  id: 1,
  email: "user1@example.org",
  name: "Developer User",
  role: "developer",
  customRoles: [],
};

const admin: TeamMember = {
  id: 2,
  email: "user2@example.org",
  name: "Admin User",
  role: "admin",
  customRoles: [],
};

const customMember: TeamMember = {
  id: 3,
  email: "user3@example.org",
  name: "Custom User",
  role: "custom",
  customRoles: [
    { id: 10, name: "Project Auditor" },
    { id: 20, name: "Billing Reader" },
  ],
};

const customRoles: CustomRoleResponse[] = [
  {
    id: 10,
    teamId: 1,
    name: "Project Auditor",
    description: null,
    statements: [],
    creator: null,
    createTime: 0,
  },
  {
    id: 20,
    teamId: 1,
    name: "Billing Reader",
    description: null,
    statements: [],
    creator: null,
    createTime: 0,
  },
  {
    id: 30,
    teamId: 1,
    name: "Deploy Operator",
    description: null,
    statements: [],
    creator: null,
    createTime: 0,
  },
];

const onSave = async () => {};
const onClose = () => {};

export const DeveloperMember: Story = {
  args: {
    team,
    member: developer,
    customRoles,
    customRolesEnabled: true,
    onSave,
    onClose,
  },
};

export const AdminMember: Story = {
  args: {
    team,
    member: admin,
    customRoles,
    customRolesEnabled: true,
    onSave,
    onClose,
  },
};

export const CustomMember: Story = {
  args: {
    team,
    member: customMember,
    customRoles,
    customRolesEnabled: true,
    onSave,
    onClose,
  },
};

export const AllCustomRolesAssigned: Story = {
  args: {
    team,
    member: {
      ...customMember,
      customRoles: [
        { id: 10, name: "Project Auditor" },
        { id: 20, name: "Billing Reader" },
        { id: 30, name: "Deploy Operator" },
      ],
    },
    customRoles,
    customRolesEnabled: true,
    onSave,
    onClose,
  },
};

export const EntitlementOff: Story = {
  args: {
    team,
    member: developer,
    customRoles: [],
    customRolesEnabled: false,
    onSave,
    onClose,
  },
};

export const NoCustomRolesExist: Story = {
  args: {
    team,
    member: { ...customMember, customRoles: [] },
    customRoles: [],
    customRolesEnabled: true,
    onSave,
    onClose,
  },
};
