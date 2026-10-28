import type { Meta, StoryObj } from "@storybook/nextjs";
import { CloudDisconnectOverlay } from "./CloudDisconnectOverlay";

const meta: Meta<typeof CloudDisconnectOverlay> = {
  component: CloudDisconnectOverlay,
  parameters: {
    layout: "fullscreen",
    a11y: { test: "todo" },
  },
};

export default meta;
type Story = StoryObj<typeof meta>;

const mockDeployment = {
  client: {} as any,
  httpClient: {} as any,
  deploymentUrl: "https://happy-animal-123.convex.cloud",
  adminKey: "mock-admin-key",
  deploymentName: "happy-animal-123",
};

const unreachableDeployment = {
  client: {} as any,
  httpClient: {} as any,
  deploymentUrl: "http://localhost:99999",
  adminKey: "mock-admin-key",
  deploymentName: "happy-animal-123",
};

export const CheckingHTTP: Story = {
  args: {
    deployment: mockDeployment,
    deploymentName: "happy-animal-123",
  },
};

export const HTTPReachable: Story = {
  args: {
    deployment: mockDeployment,
    deploymentName: "happy-animal-123",
  },
};

export const HTTPUnreachable: Story = {
  args: {
    deployment: unreachableDeployment,
    deploymentName: "happy-animal-123",
  },
};

export const NoStatusInfo: Story = {
  args: {
    deployment: unreachableDeployment,
    deploymentName: "happy-animal-123",
  },
};

export const LocalDeployment: Story = {
  args: {
    deployment: {
      ...mockDeployment,
      deploymentName: "local-happy-animal",
    },
    deploymentName: "local-happy-animal",
  },
};
