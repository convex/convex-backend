import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { ProjectEnvVarConfig } from "@common/features/settings/lib/types";
import { DefaultEnvironmentVariablesInner } from "./DefaultEnvironmentVariables";

const mockEnvironmentVariables: ProjectEnvVarConfig[] = [
  {
    name: "API_KEY",
    value: "sk_test_1234567890abcdef",
    deploymentTypes: ["dev", "preview"],
  },
  {
    name: "STRIPE_SECRET_KEY",
    value: "sk_test_abcdefghijklmnop",
    deploymentTypes: ["prod"],
  },
  {
    name: "AWS_ACCESS_KEY_ID",
    value: "AKIAIOSFODNN7EXAMPLE",
    deploymentTypes: ["dev", "preview", "prod", "custom"],
  },
];

const meta = {
  component: DefaultEnvironmentVariablesInner,
  args: {
    hasAdminPermissions: true,
    environmentVariables: mockEnvironmentVariables,
    onUpdate: fn(),
  },
} satisfies Meta<typeof DefaultEnvironmentVariablesInner>;

export default meta;
type Story = StoryObj<typeof meta>;

// Default state with variables
export const Default: Story = {};

// Empty state
export const Empty: Story = {
  args: {
    environmentVariables: [],
  },
};

// Variables with different deployment types
export const VariousDeploymentTypes: Story = {
  args: {
    environmentVariables: [
      {
        name: "DEV_ONLY_KEY",
        value: "dev-secret",
        deploymentTypes: ["dev"],
      },
      {
        name: "PREVIEW_ONLY_KEY",
        value: "preview-secret",
        deploymentTypes: ["preview"],
      },
      {
        name: "PROD_ONLY_KEY",
        value: "prod-secret",
        deploymentTypes: ["prod"],
      },
      {
        name: "ALL_ENVS_KEY",
        value: "shared-secret",
        deploymentTypes: ["dev", "preview", "prod"],
      },
    ],
  },
};

// Same name with non-overlapping deployment types (valid scenario)
export const DuplicateNamesNoConflict: Story = {
  args: {
    environmentVariables: [
      {
        name: "DATABASE_URL",
        value: "postgres://localhost:5432/dev_db",
        deploymentTypes: ["dev"],
      },
      {
        name: "DATABASE_URL",
        value: "postgres://localhost:5432/preview_db",
        deploymentTypes: ["preview"],
      },
      {
        name: "DATABASE_URL",
        value: "postgres://prod-server:5432/prod_db",
        deploymentTypes: ["prod"],
      },
    ],
  },
};

// No admin permissions
export const NoAdminPermissions: Story = {
  args: {
    hasAdminPermissions: false,
  },
};
