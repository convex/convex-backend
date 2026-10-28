import { Meta, StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import { SubscriptionCredits } from "./SubscriptionCredits";

const meta = {
  component: SubscriptionCredits,
  render: (args) => (
    // Render inside a Sheet like it appears on the billing page, as a line item
    // under the subscription details.
    <Sheet className="flex flex-col gap-4">
      <h3>Subscription</h3>
      <SubscriptionCredits {...args} />
    </Sheet>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof SubscriptionCredits>;

export default meta;
type Story = StoryObj<typeof meta>;

// Renders nothing when the customer has no account balance.
export const Empty: Story = {
  args: {
    accountBalance: null,
  },
};

export const WithBalance: Story = {
  args: {
    accountBalance: "42.50",
  },
};

// A zero balance is treated as "nothing to report" and is not shown.
export const ZeroBalance: Story = {
  args: {
    accountBalance: "0.00",
  },
};
