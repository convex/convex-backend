import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, fn } from "storybook/test";
import { PlatformDeployKeyResponse } from "@convex-dev/platform/managementApi";
import { ProjectSettingsPage } from "../../../pages/t/[team]/[project]/settings";
import { useUpdateProject } from "api/projects";
import {
  useCurrentProjectRoles,
  useProjectRoles,
  useUpdateProjectRoles,
} from "api/roles";
import {
  useCreatePreviewDeployKey,
  usePreviewDeployKeys,
  useProjectAppAccessTokens,
  useDeleteAppAccessTokenByName,
} from "api/accessTokens";
import {
  useProjectEnvironmentVariables,
  useUpdateProjectEnvVars,
} from "api/environmentVariables";
import { useBBMutation, useManagementApiMutation } from "api/api";

const mockProjectRoles = [
  {
    memberId: 1,
    projectId: 7,
    role: "admin",
  },
];

const mockDeployKey: PlatformDeployKeyResponse = {
  id: 1,
  name: "storybook-preview-key",
  creator: 1,
  creationTime: Date.now() - 1000 * 60 * 60,
  lastUsedTime: Date.now() - 1000 * 60 * 30,
  // Unrestricted keys list every deployment action.
  allowedActions: [
    "deployment:deploy",
    "deployment:env:view",
    "deployment:env:write",
    "deployment:pause",
    "deployment:unpause",
    "deployment:logs:view",
    "deployment:metrics:view",
    "deployment:integrations:view",
    "deployment:integrations:write",
    "deployment:data:view",
    "deployment:data:write",
    "deployment:backups:view",
    "deployment:backups:create",
    "deployment:backups:download",
    "deployment:backups:delete",
    "deployment:backups:import",
    "deployment:functions:actAsUser",
    "deployment:functions:runInternalQueries",
    "deployment:functions:runInternalMutations",
    "deployment:functions:runInternalActions",
    "deployment:functions:runTestQuery",
    "deployment:auditLog:view",
  ],
};

const mockAppToken: NonNullable<
  ReturnType<typeof useProjectAppAccessTokens>
>[number] = {
  id: 2,
  appClientId: "storybook-app-client-id",
  appName: "Storybook OAuth App",
  name: "storybook-preview-key",
  creationTime: Date.now() - 1000 * 60 * 60,
  lastUsedTime: Date.now() - 1000 * 60 * 30,
};

function applyProjectSettingsMocks() {
  mocked(useProjectRoles).mockReturnValue({
    isLoading: false,
    projectRoles: mockProjectRoles as any,
  });
  mocked(useCurrentProjectRoles).mockReturnValue(mockProjectRoles as any);
  mocked(useUpdateProjectRoles).mockReturnValue(fn());
  mocked(useUpdateProject).mockReturnValue(fn());
  mocked(useProjectEnvironmentVariables).mockReturnValue({
    configs: [],
  });
  mocked(useUpdateProjectEnvVars).mockReturnValue(fn());
  mocked(useCreatePreviewDeployKey).mockReturnValue(fn());
  mocked(usePreviewDeployKeys).mockReturnValue([mockDeployKey]);
  mocked(useProjectAppAccessTokens).mockReturnValue([mockAppToken]);
  mocked(useDeleteAppAccessTokenByName).mockReturnValue(fn());
  mocked(useBBMutation).mockReturnValue(fn());
  mocked(useManagementApiMutation).mockReturnValue(fn());
}

const meta = {
  component: ProjectSettingsPage,
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/settings",
        route: "/t/[team]/[project]/settings",
        basePath: `/t/acme/my-amazing-app/settings`,
        asPath: `/t/acme/my-amazing-app/settings`,
        query: {
          team: "acme",
          project: "my-amazing-app",
        },
      },
    },
    a11y: {
      test: "todo",
    },
  },
  beforeEach: () => {
    applyProjectSettingsMocks();
  },
} satisfies Meta<typeof ProjectSettingsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
