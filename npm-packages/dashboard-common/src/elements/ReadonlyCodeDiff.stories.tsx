import { Meta, StoryObj } from "@storybook/nextjs";
import { ReadonlyCodeDiff } from "@common/elements/ReadonlyCode";

const meta = {
  component: ReadonlyCodeDiff,
  // Monaco loads asynchronously, so whether axe sees the editor varies from
  // run to run, and the loaded editor has violations of its own.
  parameters: { a11y: { test: "todo" } },
  args: {
    language: "javascript",
    path: "example.js",
  },
  render: (args) => (
    <div style={{ height: "100vh", background: "#eee" }}>
      <ReadonlyCodeDiff {...args} />
    </div>
  ),
} satisfies Meta<typeof ReadonlyCodeDiff>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ParentHeight: Story = {
  args: {
    originalCode:
      "This line is removed on the right.\njust some text\nabcd\nefgh\nSome more text",
    modifiedCode:
      "just some text\nabcz\nzzzzefgh\nSome more text.\nThis line is removed on the left.",
  },
};

export const ContentHeight: Story = {
  args: {
    language: "plaintext",
    originalCode:
      "Hello world\nHello world\nHello world\nHello world\nHello world\nHello world\nHelloworld\nHello world\n",
    modifiedCode:
      "Hi world\nHello world\nHello world\nHello world\nHello world\nHello world\nHello world\nHello  world\nHello world\nHello world",
    height: { type: "content" },
  },
};
