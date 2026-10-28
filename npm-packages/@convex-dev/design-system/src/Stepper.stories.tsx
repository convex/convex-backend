import { Meta, StoryObj } from "@storybook/nextjs";
import { useState } from "react";
import { Stepper } from "./Stepper";
import { Sheet } from "@ui/Sheet";

const meta = {
  component: Stepper,
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
} satisfies Meta<typeof Stepper>;

export default meta;
type Story = StoryObj<typeof meta>;

function Interactive({
  stepLabels,
  defaultStep = 0,
}: {
  stepLabels: string[];
  defaultStep?: number;
}) {
  const [activeStep, setActiveStep] = useState(defaultStep);
  return (
    <Stepper activeStep={activeStep} onSelectStep={setActiveStep}>
      {stepLabels.map((label) => (
        <Stepper.Step key={label} label={label}>
          <div className="rounded-sm border border-border-transparent p-4 text-sm text-content-secondary">
            Content for "{label}"
          </div>
        </Stepper.Step>
      ))}
    </Stepper>
  );
}

const threeStepLabels = [
  "Billing Information",
  "Spending Limits",
  "Payment Information",
];

export const ThreeSteps: Story = {
  args: { activeStep: 0, children: null },
  render: () => <Interactive stepLabels={threeStepLabels} />,
};

export const MiddleStep: Story = {
  args: { activeStep: 1, children: null },
  render: () => <Interactive stepLabels={threeStepLabels} defaultStep={1} />,
};

export const LastStep: Story = {
  args: { activeStep: 2, children: null },
  render: () => <Interactive stepLabels={threeStepLabels} defaultStep={2} />,
};

export const TwoSteps: Story = {
  args: { activeStep: 0, children: null },
  render: () => <Interactive stepLabels={["Account Setup", "Confirmation"]} />,
};
