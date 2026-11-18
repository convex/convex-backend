import { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import type { TeamMember } from "@convex-dev/platform/managementApi";
import type { ProjectDetails } from "generatedApi";
import { ProjectAdminFormModal } from "./ProjectAdminsFormModal";

const project = {
  id: 7,
  name: "my-amazing-app",
  slug: "my-amazing-app",
  teamId: 1,
} as ProjectDetails;

const names = [
  "Ada Lovelace",
  "Alan Turing",
  "Barbara Liskov",
  "Claude Shannon",
  "Donald Knuth",
  "Edsger Dijkstra",
  "Frances Allen",
  "Grace Hopper",
  "John Backus",
  "Ken Thompson",
  "Leslie Lamport",
  "Margaret Hamilton",
  "Niklaus Wirth",
  "Radia Perlman",
];

const members: TeamMember[] = [
  ...names.map((name, i) => ({
    id: i + 1,
    name,
    email: `${name.split(" ")[0].toLowerCase()}@example.com`,
    role: i === 0 ? ("admin" as const) : ("developer" as const),
  })),
  {
    id: 100,
    email: "no-name-member@example.com",
    role: "developer" as const,
  },
] as TeamMember[];

const meta = {
  component: ProjectAdminFormModal,
  args: {
    project,
    members,
    projectRoles: [
      { memberId: 2, projectId: 7, teamId: 1, role: "admin", lastUpdated: 0 },
      { memberId: 5, projectId: 7, teamId: 1, role: "admin", lastUpdated: 0 },
    ],
    onUpdateProjectRoles: fn(),
    onClose: fn(),
  },
} satisfies Meta<typeof ProjectAdminFormModal>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const FewMembers: Story = {
  args: {
    members: members.slice(0, 3),
  },
};
