import type { Meta, StoryObj } from "@storybook/nextjs";
import { userEvent, within } from "storybook/test";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { DeploymentEnvironmentVariables } from "@common/features/settings/components/DeploymentEnvironmentVariables";
import { GenericId } from "convex/values";

const mockDeployment = {
  id: 11,
  name: "happy-capybara-123",
  deploymentType: "prod" as const,
  kind: "cloud" as const,
  isDefault: true,
  projectId: 7,
  creator: 1,
  createTime: Date.now(),
  class: "s256",
  deploymentUrl: "https://happy-capybara-123.convex.cloud",
  reference: "production",
  region: "aws-us-east-1",
} as const;

const mockClient = mockConvexReactClient().registerQueryFake(
  udfs.listEnvironmentVariables.default,
  () => [
    {
      _id: "k8envaaaaaaaaaaaaaaaaaaaaaaaaaaa" as GenericId<"_environment_variables">,
      _creationTime: Date.now(),
      name: "API_KEY",
      value: "sk_test_1234567890abcdef",
    },
    {
      _id: "k8envbbbbbbbbbbbbbbbbbbbbbbbbbbb" as GenericId<"_environment_variables">,
      _creationTime: Date.now(),
      name: "STRIPE_SECRET_KEY",
      value: "sk_test_abcdefghijklmnop",
    },
    {
      _id: "k8envccccccccccccccccccccccccccc" as GenericId<"_environment_variables">,
      _creationTime: Date.now(),
      name: "API_URL",
      value: "https://api.example.com",
    },
  ],
);

const mockConnectedDeployment = {
  deployment: {
    client: mockClient,
    httpClient: {} as never,
    deploymentUrl: mockDeployment.deploymentUrl,
    adminKey: "storybook-admin-key",
    deploymentName: mockDeployment.name,
  },
  isDisconnected: false,
};

const defaultDeploymentInfoValue = {
  ...mockDeploymentInfo,
  useCurrentDeployment: () => mockDeployment,
  useHasProjectAdminPermissions: () => true,
  useIsOperationAllowed: () => true,
  useProjectEnvironmentVariables: () => undefined,
};

const meta = {
  component: DeploymentEnvironmentVariables,
  render: () => (
    <ConnectedDeploymentContext.Provider value={mockConnectedDeployment}>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider value={defaultDeploymentInfoValue}>
          <div className="max-w-2xl">
            <DeploymentEnvironmentVariables />
          </div>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof DeploymentEnvironmentVariables>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithDefaultDiff: Story = {
  render: () => (
    <ConnectedDeploymentContext.Provider value={mockConnectedDeployment}>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider
          value={{
            ...defaultDeploymentInfoValue,
            useProjectEnvironmentVariables: () => ({
              configs: [
                {
                  name: "API_KEY",
                  value: "different_value",
                  deploymentTypes: ["prod"],
                },
              ],
            }),
          }}
        >
          <div className="max-w-2xl">
            <DeploymentEnvironmentVariables />
          </div>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
};

export const EditInline: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Add" }));
    await userEvent.type(
      canvas.getByRole("textbox", { name: "Name" }),
      "NEW_VAR",
    );
    await userEvent.type(
      canvas.getByRole("textbox", { name: "Value" }),
      "my_secret_value",
    );
  },
};
