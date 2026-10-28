import { Meta, StoryObj } from "@storybook/nextjs";
import { ComponentProps, useState } from "react";
import { Button } from "@ui/Button";
import { CopiedPopper } from "@common/elements/CopiedPopper";

const meta = {
  component: CopiedPopper,
  args: {
    // Overridden by the Example wrapper, which wires up the real elements.
    referenceElement: null,
    copiedPopperElement: null,
    setCopiedPopperElement: () => {},
    show: true,
  },
  render: (args) => <Example {...args} />,
  parameters: {
    a11y: { test: "todo" },
  },
} satisfies Meta<typeof CopiedPopper>;

export default meta;
type Story = StoryObj<typeof meta>;

function Example(args: ComponentProps<typeof CopiedPopper>) {
  const [referenceElement, setReferenceElement] = useState<HTMLElement | null>(
    null,
  );
  const [copiedPopperElement, setCopiedPopperElement] =
    useState<HTMLDivElement | null>(null);

  return (
    <div className="flex justify-center p-24">
      <Button ref={setReferenceElement} variant="neutral">
        Reference element
      </Button>
      <CopiedPopper
        {...args}
        referenceElement={referenceElement}
        copiedPopperElement={copiedPopperElement}
        setCopiedPopperElement={setCopiedPopperElement}
      />
    </div>
  );
}

export const Primary: Story = {};

export const CustomMessage: Story = {
  args: {
    message: "Copied to clipboard",
  },
};

export const TopPlacement: Story = {
  args: {
    placement: "top",
  },
};
