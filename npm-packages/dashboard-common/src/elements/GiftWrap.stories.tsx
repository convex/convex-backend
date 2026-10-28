import { Meta, StoryObj } from "@storybook/nextjs";
import { ReactNode, useState } from "react";
import { Button } from "@ui/Button";
import { GiftParcel, GiftWrap } from "./GiftWrap";

function Box({ children }: { children?: ReactNode }) {
  return (
    <div className="flex min-w-0 grow flex-wrap items-center gap-1.5 rounded-md bg-background-secondary p-2 font-mono text-xs">
      {children ?? <span className="text-content-tertiary">content</span>}
    </div>
  );
}

function Example({
  explanation,
  initialOpened,
  tall,
}: {
  explanation?: string;
  initialOpened: boolean;
  tall?: boolean;
}) {
  const [opened, setOpened] = useState(initialOpened);
  return (
    // The explanation bubble is centred on the parcel and can be 20rem wide, so
    // the left padding keeps it off the canvas edge when the parcel is narrow.
    <div className="w-xl max-w-full space-y-2 p-4 pl-40">
      <GiftWrap
        className="min-w-0"
        explanation={explanation}
        opened={opened}
        onOpen={() => setOpened(true)}
      >
        <Box>
          {tall && (
            <>
              <span>row one</span>
              <span>row two</span>
              <span>row three</span>
              <span>row four</span>
              <span>row five</span>
              <span>row six</span>
            </>
          )}
        </Box>
      </GiftWrap>
      {opened && (
        <Button
          size="xs"
          variant="neutral"
          onClick={() => setOpened(false)}
          icon={<GiftParcel className="size-4" />}
        >
          Wrap it up again
        </Button>
      )}
    </div>
  );
}

const meta = {
  component: Example,
  parameters: {
    a11y: { test: "todo" },
  },
} satisfies Meta<typeof Example>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Wrapped: Story = {
  args: {
    initialOpened: false,
    explanation: "Unwrap to see what's inside.",
  },
};

export const NoExplanation: Story = { args: { initialOpened: false } };

/** The bow keeps the same size as the box grows taller. */
export const Tall: Story = { args: { initialOpened: false, tall: true } };

export const Unwrapped: Story = { args: { initialOpened: true } };

/** Static gift icon (paper, band, bow) with no animation. */
export const Parcel: StoryObj = {
  render: () => (
    <div className="flex items-end gap-4 p-4">
      <GiftParcel />
      <GiftParcel className="size-8.5 rounded-none border-0" />
    </div>
  ),
};
