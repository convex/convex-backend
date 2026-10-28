import type { Meta, StoryObj } from "@storybook/nextjs";
import { useState } from "react";
import { ToastContainer } from "@common/elements/ToastContainer";
import { toast } from "@common/lib/utils";
import { Sheet } from "@ui/Sheet";
import { TextInput } from "@ui/TextInput";
import { Button } from "@ui/Button";

const meta = {
  component: ToastContainer,
} satisfies Meta<typeof ToastContainer>;

export default meta;
type Story = StoryObj<typeof meta>;

function ToastDemo() {
  const [message, setMessage] = useState("Hello world");

  const types = ["success", "error", "info", "warning"] as const;
  const [type, setType] = useState<(typeof types)[number]>("success");
  const [permanent, setPermanent] = useState(false);

  return (
    <Sheet className="text-sm">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          toast(type, message, undefined, permanent ? false : undefined);
        }}
      >
        <div className="flex flex-col items-start gap-4">
          <TextInput
            id="toast"
            value={message}
            onChange={(e) => setMessage(e.currentTarget.value)}
          />

          <div className="flex flex-wrap gap-4">
            {types.map((t) => (
              <label key={t}>
                <input
                  type="radio"
                  className="mr-2"
                  checked={type === t}
                  onChange={() => setType(t)}
                />
                {t}
              </label>
            ))}
          </div>

          <label>
            <input
              className="mr-1"
              type="checkbox"
              checked={permanent}
              onChange={(e) => {
                setPermanent(e.target.checked);
              }}
            />{" "}
            Permanent
          </label>

          <Button type="submit">Show Toast</Button>
        </div>
      </form>
    </Sheet>
  );
}

export const Demo: Story = {
  render: () => <ToastDemo />,
};
