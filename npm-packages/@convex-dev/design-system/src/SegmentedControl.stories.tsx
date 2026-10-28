import { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { useState } from "react";
import { SegmentedControl, SegmentedControlOption } from "./SegmentedControl";

const meta = {
  component: SegmentedControl,
  args: {
    onChange: fn(),
  },
} satisfies Meta<typeof SegmentedControl>;

export default meta;
type Story = StoryObj<typeof meta>;

function Interactive<T extends string>({
  options,
  defaultValue,
}: {
  options: SegmentedControlOption<T>[];
  defaultValue: T;
}) {
  const [value, setValue] = useState(defaultValue);
  return (
    <SegmentedControl options={options} value={value} onChange={setValue} />
  );
}

export const TwoOptions: Story = {
  args: {
    options: [
      { label: "Projects", value: "projects" },
      { label: "Deployments", value: "deployments" },
    ],
    value: "projects",
  },
  render: () => (
    <Interactive
      options={[
        { label: "Projects", value: "projects" },
        { label: "Deployments", value: "deployments" },
      ]}
      defaultValue="projects"
    />
  ),
};

export const TwoOptionsSecondSelected: Story = {
  args: {
    options: [
      { label: "Projects", value: "projects" },
      { label: "Deployments", value: "deployments" },
    ],
    value: "deployments",
  },
  render: () => (
    <Interactive
      options={[
        { label: "Projects", value: "projects" },
        { label: "Deployments", value: "deployments" },
      ]}
      defaultValue="deployments"
    />
  ),
};

export const ThreeOptions: Story = {
  args: {
    options: [
      { label: "Day", value: "day" },
      { label: "Week", value: "week" },
      { label: "Month", value: "month" },
    ],
    value: "week",
  },
  render: () => (
    <Interactive
      options={[
        { label: "Day", value: "day" },
        { label: "Week", value: "week" },
        { label: "Month", value: "month" },
      ]}
      defaultValue="week"
    />
  ),
};

export const FourOptions: Story = {
  args: {
    options: [
      { label: "All", value: "all" },
      { label: "Success", value: "success" },
      { label: "Error", value: "error" },
      { label: "Pending", value: "pending" },
    ],
    value: "all",
  },
  render: () => (
    <Interactive
      options={[
        { label: "All", value: "all" },
        { label: "Success", value: "success" },
        { label: "Error", value: "error" },
        { label: "Pending", value: "pending" },
      ]}
      defaultValue="all"
    />
  ),
};

export const FourOptionsConstrained: Story = {
  args: {
    options: [
      { label: "Function Calls", value: "function_calls" },
      { label: "Database Bandwidth", value: "database_bandwidth" },
      { label: "Action Compute", value: "action_compute" },
      { label: "Vector Bandwidth", value: "vector_bandwidth" },
    ],
    value: "function_calls",
  },
  render: () => (
    <div
      style={{
        resize: "horizontal",
        overflow: "hidden",
        maxWidth: 1000,
        width: 400,
        border: "1px dashed #ccc",
        padding: 8,
      }}
    >
      <Interactive
        options={[
          { label: "Function Calls", value: "function_calls" },
          { label: "Database Bandwidth", value: "database_bandwidth" },
          { label: "Action Compute", value: "action_compute" },
          { label: "Vector Bandwidth", value: "vector_bandwidth" },
        ]}
        defaultValue="function_calls"
      />
    </div>
  ),
};
