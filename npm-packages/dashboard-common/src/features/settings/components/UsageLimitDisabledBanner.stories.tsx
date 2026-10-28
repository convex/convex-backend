import type { Meta, StoryObj } from "@storybook/nextjs";
import { UsageLimitDisabledBanner } from "./UsageLimitDisabledBanner";

const meta = {
  component: UsageLimitDisabledBanner,
  args: {
    usageLimitsUri: "#",
  },
} satisfies Meta<typeof UsageLimitDisabledBanner>;

export default meta;
type Story = StoryObj<typeof meta>;

// Shown when the deployment's usage-limit stop state is "disabled".
export const Default: Story = {};
