import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { DeploymentReference } from "./DeploymentReference";

const meta: Meta<typeof DeploymentReference> = {
  component: DeploymentReference,
  args: {
    value: "dev/nicolas",
    canManage: true,
    onUpdate: fn(),
  },
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    value: "dev/nicolas",
  },
};

export const LongRef: Story = {
  args: {
    value:
      "preview/implement-super-long-descriptive-branch-name-for-new-awesome-feature-with-extended-scope-v1x",
  },
};

export const SubmissionPending: Story = {
  args: {
    async onUpdate() {
      await new Promise((resolve) => {
        setTimeout(resolve, 2000);
      });
    },
  },
};

export const SubmissionFailing = {
  args: {
    async onUpdate() {
      await new Promise((resolve) => {
        setTimeout(resolve, 500);
      });
      throw new Error("This reference is already used.");
    },
  },
};

export const CanNotManage: Story = {
  args: {
    canManage: false,
  },
};
