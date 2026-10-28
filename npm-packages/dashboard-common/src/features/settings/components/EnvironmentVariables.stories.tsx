import type { Meta, StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import { fn } from "storybook/test";
import {
  EnvironmentVariables,
  BaseEnvironmentVariable,
} from "./EnvironmentVariables";

const meta = {
  component: EnvironmentVariables,
  args: {
    updateEnvironmentVariables: fn(),
    hasAdminPermissions: true,
    initEnvVar: (envVar) => envVar,
    environmentVariables: [
      { name: "API_KEY", value: "sk_test_1234567890abcdef" },
      { name: "STRIPE_SECRET_KEY", value: "sk_test_abcdefghijklmnop" },
      { name: "AWS_ACCESS_KEY_ID", value: "AKIAIOSFODNN7EXAMPLE" },
      {
        name: "AWS_SECRET_ACCESS_KEY",
        value: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
      },
      {
        name: "SENDGRID_API_KEY",
        value: "SG.1234567890abcdefghijklmnopqrstuvwxyz",
      },
      { name: "JWT_SECRET", value: "super-secret-jwt-key-that-should-be-long" },
      { name: "GITHUB_CLIENT_ID", value: "Iv1.1234567890abcdef" },
    ],
  },
  render: (args) => (
    <Sheet>
      <EnvironmentVariables {...args} />
    </Sheet>
  ),
} satisfies Meta<typeof EnvironmentVariables<BaseEnvironmentVariable>>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {
  args: {
    environmentVariables: [],
  },
};

export const WithVariables: Story = {};

export const WithInitialFormValues: Story = {
  args: {
    initialFormValues: [
      { name: "NEW_VAR", value: "new_value" },
      { name: "", value: "" },
    ],
  },
};

export const WithWhitespaceValues: Story = {
  args: {
    environmentVariables: [],
    initialFormValues: [
      { name: "EMPTY_STRING", value: "" },
      { name: "LEADING_SPACE", value: "  value" },
      { name: "TRAILING_SPACE", value: "value  " },
      { name: "BOTH_SPACES", value: "  value  " },
      { name: "LINE_RETURNS", value: "hello\nworld" },
      { name: "INTERNAL_SPACES", value: "value with spaces" },
    ],
  },
};

export const WithQuotedValues: Story = {
  args: {
    environmentVariables: [],
    initialFormValues: [
      { name: "QUOTED_VALUE", value: '"value in quotes"' },
      { name: "SINGLE_QUOTED", value: "'value in single quotes'" },
      { name: "NORMAL_VALUE", value: "value without quotes" },
    ],
  },
};

export const NoAdminPermissions: Story = {
  args: {
    hasAdminPermissions: false,
  },
};

export const WithCopyFormattingWarnings: Story = {
  args: {
    environmentVariables: [
      { name: "CRLF_VAR_ONE", value: "line1\r\nline2" },
      { name: "CRLF_VAR_TWO", value: "line3\rline4" },
    ],
  },
};

export const AtLimit: Story = {
  args: {
    environmentVariables: Array.from({ length: 100 }, (_, i) => ({
      name: `VAR_${i + 1}`,
      value: `value_${i + 1}`,
    })),
  },
};
