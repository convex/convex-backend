import { Meta, StoryObj } from "@storybook/nextjs";
import { DailyPerTagMetrics } from "hooks/usageMetrics";
import { Sheet } from "@ui/Sheet";
import { useState } from "react";
import { UsageStackedBarChart } from "./UsageBarChart";

function UsageStackedBarChartWrapper(
  args: Omit<
    React.ComponentProps<typeof UsageStackedBarChart>,
    "selectedDate" | "setSelectedDate"
  >,
) {
  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  return (
    <UsageStackedBarChart
      {...args}
      selectedDate={selectedDate}
      setSelectedDate={setSelectedDate}
    />
  );
}

const meta = {
  component: UsageStackedBarChartWrapper,
  args: {
    categories: {
      cats: {
        name: "Cats",
        color: "fill-purple-200 dark:fill-purple-800",
      },
      dogs: {
        name: "Dogs",
        color: "fill-orange-200 dark:fill-orange-800",
      },
    },
  },
  render: (args) => (
    <Sheet>
      <h3 className="mb-4">Chart</h3>
      <UsageStackedBarChartWrapper {...args} />
    </Sheet>
  ),
} satisfies Meta<typeof UsageStackedBarChartWrapper>;

export default meta;
type Story = StoryObj<typeof meta>;

const rows: DailyPerTagMetrics[] = [...Array(31).keys()].map((dayIndex) => ({
  ds: `2023-07-${(dayIndex + 1).toString().padStart(2, "0")}`,
  metrics: ["cats", "dogs", "puppies"].map((tag) => ({
    tag,
    value: Math.floor(Math.random() * 100000),
  })),
  categoryRenames: { puppies: "dogs" },
}));

export const Standard: Story = {
  args: {
    rows: rows.slice(0, 15),
  },
};

export const FullMonth: Story = {
  args: {
    rows,
  },
};

export const FewDays: Story = {
  args: {
    rows: rows.slice(0, 3),
  },
};

export const SingleEntry: Story = {
  args: {
    rows: rows.slice(0, 1),
  },
};

export const Empty: Story = {
  args: {
    rows: [],
  },
};

export const MissingEntries: Story = {
  args: {
    rows: [...rows.slice(20, 30), ...rows.slice(0, 10)],
  },
};

export const Storage: Story = {
  args: {
    rows,
    quantityType: "storage",
  },
};

export const ActionCompute: Story = {
  args: {
    rows,
    quantityType: "actionCompute",
  },
};
