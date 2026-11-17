import type { Meta, StoryObj } from "@storybook/nextjs";
import { TeamResponse } from "generatedApi";
import {
  PlatformDeploymentResponse,
  ProjectResponse,
} from "@convex-dev/platform/managementApi";
import { ProjectCard } from "components/projects/ProjectCard";
import { useProjectById } from "api/projects";
import { useDeploymentByName } from "api/deployments";
import { useCurrentTeam } from "api/teams";
import { useProfile } from "api/profile";
import { useHasProjectAdminPermissions, useMyCustomRoles } from "api/roles";
import { mocked, userEvent, within } from "storybook/test";
import { cn } from "@ui/cn";

const mockTeam: TeamResponse = {
  id: 2,
  creator: 1,
  slug: "acme",
  name: "Acme Corp",
  suspended: false,
  referralCode: "ACME01",
  referredBy: null,
};

const project: ProjectResponse = {
  id: 7,
  teamId: mockTeam.id,
  teamSlug: mockTeam.slug,
  name: "My amazing app",
  slug: "my-amazing-app",
  createTime: Date.now(),
  tags: [],
  prodDeploymentName: "musical-otter-456",
  devDeploymentName: "happy-capybara-123",
  prodDeploymentReference: "production",
  devDeploymentReference: "dev/jane-doe",
};

const meta = {
  component: ProjectCard,
  args: {
    project,
  },
  beforeEach: () => {
    mocked(useCurrentTeam).mockReturnValue(mockTeam);
    mocked(useProjectById).mockReturnValue({
      project,
      isLoading: false,
      error: undefined,
    });
    mocked(useDeploymentByName).mockReturnValue({
      deployment: undefined,
      notFound: false,
    });
    mocked(useHasProjectAdminPermissions).mockReturnValue(true);
    mocked(useMyCustomRoles).mockReturnValue({
      role: "admin",
      customRoles: [],
    });
    mocked(useProfile).mockReturnValue({
      id: 1,
      name: "Jane Doe",
      email: "jane@acme.dev",
    });
  },
  render: (args) => (
    <div className="w-full max-w-sm">
      <ProjectCard {...args} />
    </div>
  ),
} satisfies Meta<typeof ProjectCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      canvas.getByRole("button", { name: "Open project settings" }),
    );
  },
};

const previewDeployment: PlatformDeploymentResponse = {
  kind: "cloud",
  id: 321,
  name: "quiet-heron-321",
  createTime: Date.now(),
  deploymentType: "preview",
  projectId: project.id,
  region: "aws-us-east-1",
  isDefault: false,
  reference: "preview/fix-login-redirect",
  previewIdentifier: "fix-login-redirect",
  deploymentUrl: "https://quiet-heron-321.convex.cloud",
  class: "s16",
};

export const WithRecentDeployment: Story = {
  // Wide enough that none of the deployment pills truncate.
  render: (args) => (
    <div className="w-full max-w-xl">
      <ProjectCard {...args} />
    </div>
  ),
  beforeEach: () => {
    mocked(useDeploymentByName).mockReturnValue({
      deployment: previewDeployment,
      notFound: false,
    });
    const key = `/lastViewedNonDefaultDeploymentForProject/${project.id}`;
    window.localStorage.setItem(
      key,
      JSON.stringify({
        name: previewDeployment.name,
        reference: previewDeployment.reference,
        deploymentType: previewDeployment.deploymentType,
      }),
    );
    return () => window.localStorage.removeItem(key);
  },
};

// A spread of states for reviewing the layout: tag overflow, long names and
// tags, untitled projects, and projects missing a default deployment.
const gallery: ProjectResponse[] = [
  { ...project, id: 101, tags: ["frontend", "billing"] },
  {
    ...project,
    id: 102,
    name: "Analytics pipeline",
    slug: "analytics-pipeline",
    createTime: Date.now() - 60 * 24 * 60 * 60 * 1000,
    tags: ["data", "internal", "etl", "batch-jobs", "on-call-team-b"],
    devDeploymentName: "local-jane-doe",
    devDeploymentReference: null,
  },
  {
    ...project,
    id: 103,
    name: "Marketing site",
    slug: "marketing-site",
    createTime: Date.now() - 365 * 24 * 60 * 60 * 1000,
    devDeploymentName: null,
    devDeploymentReference: null,
  },
  {
    ...project,
    id: 104,
    name: "",
    slug: "wandering-otter-832",
    tags: ["scratch"],
    prodDeploymentName: null,
    prodDeploymentReference: null,
  },
  {
    ...project,
    id: 105,
    name: "Customer support portal with a really long project name",
    slug: "customer-support-portal-with-a-really-long-name",
    createTime: Date.now() - 240 * 24 * 60 * 60 * 1000,
    tags: ["customer-facing-production-apps", "support"],
    devDeploymentReference: "dev/customer-success-engineering",
  },
];

function mockGallery() {
  mocked(useProjectById).mockImplementation((projectId) => ({
    project: gallery.find((p) => p.id === projectId),
    isLoading: false,
    error: undefined,
  }));
}

export const Grid: Story = {
  beforeEach: mockGallery,
  render: () => (
    <div className="grid w-full grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
      {gallery.map((p) => (
        <ProjectCard key={p.id} project={p} />
      ))}
    </div>
  ),
};

export const List: Story = {
  beforeEach: mockGallery,
  render: () => (
    <div className="w-full overflow-hidden rounded-xl bg-background-secondary ring-1 ring-border-transparent">
      {gallery.map((p, i) => (
        <div
          key={p.id}
          className={cn(
            "first:rounded-t-xl last:rounded-b-xl",
            i > 0 && "border-t",
          )}
        >
          <ProjectCard project={p} listItem />
        </div>
      ))}
    </div>
  ),
};

// Custom-role members only see the default deployments their role can view.
// One that can view neither falls back to the project's deployments list.
const customRoleGallery: ProjectResponse[] = [
  { ...project, id: 201, name: "Can view both defaults" },
  {
    ...project,
    id: 202,
    name: "Can view production only",
    devDeploymentName: null,
    devDeploymentReference: null,
  },
  {
    ...project,
    id: 203,
    name: "Can view neither default",
    prodDeploymentName: null,
    prodDeploymentReference: null,
    devDeploymentName: null,
    devDeploymentReference: null,
  },
];

function mockCustomRoleGallery() {
  mocked(useMyCustomRoles).mockReturnValue({ role: "custom", customRoles: [] });
  mocked(useHasProjectAdminPermissions).mockReturnValue(false);
  mocked(useProjectById).mockImplementation((projectId) => ({
    project: customRoleGallery.find((p) => p.id === projectId),
    isLoading: false,
    error: undefined,
  }));
}

export const CustomRoleGrid: Story = {
  beforeEach: mockCustomRoleGallery,
  render: () => (
    <div className="grid w-full grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
      {customRoleGallery.map((p) => (
        <ProjectCard key={p.id} project={p} />
      ))}
    </div>
  ),
};

export const CustomRoleList: Story = {
  beforeEach: mockCustomRoleGallery,
  render: () => (
    <div className="w-full overflow-hidden rounded-xl bg-background-secondary ring-1 ring-border-transparent">
      {customRoleGallery.map((p, i) => (
        <div
          key={p.id}
          className={cn(
            "first:rounded-t-xl last:rounded-b-xl",
            i > 0 && "border-t",
          )}
        >
          <ProjectCard project={p} listItem />
        </div>
      ))}
    </div>
  ),
};
