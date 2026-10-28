import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { DefaultEnvironmentVariablesInner } from "components/projectSettings/DefaultEnvironmentVariables";

const meta = {
  component: DefaultEnvironmentVariablesInner,
  args: {
    environmentVariables: [
      {
        name: "API_URL",
        value: "https://api.example.com",
        deploymentTypes: ["prod"],
      },
      {
        name: "API_URL",
        value: "https://staging.api.example.com",
        deploymentTypes: ["dev", "preview"],
      },
    ],
    onUpdate: fn(),
    hasAdminPermissions: true,
  },
  render: (args) => (
    <div className="max-w-2xl">
      <DefaultEnvironmentVariablesInner {...args} />
    </div>
  ),
} satisfies Meta<typeof DefaultEnvironmentVariablesInner>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
