import { Meta, StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import { PrepaidCredits } from "./PrepaidCredits";

const meta = {
  component: PrepaidCredits,
  render: (args) => (
    // Render inside a Sheet like it appears on the billing page, as a section
    // between the subscription details and the spending limits.
    <Sheet className="flex flex-col gap-4">
      <PrepaidCredits {...args} />
      <h4>Usage Spending Limits</h4>
    </Sheet>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof PrepaidCredits>;

export default meta;
type Story = StoryObj<typeof meta>;

const NOVEMBER_2026 = new Date("2026-11-06T00:00:00Z").getTime();
const JANUARY_2027 = new Date("2027-01-06T00:00:00Z").getTime();

export const PartiallyConsumed: Story = {
  args: {
    credits: [
      {
        id: "block_1",
        itemName: "Business Plan Minimum",
        description: null,
        balance: 1_500,
        initialBalance: 2_500,
        expiryDate: NOVEMBER_2026,
      },
    ],
  },
};

export const SeveralCredits: Story = {
  args: {
    credits: [
      // Fully drawn down, but not expired yet.
      {
        id: "block_1",
        itemName: "Business Plan Minimum",
        description: null,
        balance: 0,
        initialBalance: 2_500,
        expiryDate: NOVEMBER_2026,
      },
      // Granted by hand, so it's annotated but has no allocation behind it.
      {
        id: "block_2",
        itemName: null,
        description: "AI Gateway launch promo",
        balance: 25,
        initialBalance: 50,
        expiryDate: null,
      },
      // Both labels: a hand-annotated top-up against a named allocation.
      {
        id: "block_4",
        itemName: "Included Allocation (USD)",
        description: "Q3 goodwill top-up",
        balance: 1_000,
        initialBalance: 1_000,
        expiryDate: JANUARY_2027,
      },
    ],
  },
};
