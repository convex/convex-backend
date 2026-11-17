import { Meta, StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import { useState } from "react";
import { userEvent, within } from "storybook/test";
import { ProjectTagsInput } from "./ProjectTagsInput";

const meta = {
  component: ProjectTagsInput,
  args: {
    tags: ["backend", "production"],
    onChange: () => {},
  },
  render: function Render(args) {
    const [tags, setTags] = useState(args.tags);
    return (
      <Sheet className="max-w-sm">
        <ProjectTagsInput {...args} tags={tags} onChange={setTags} />
      </Sheet>
    );
  },
} satisfies Meta<typeof ProjectTagsInput>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Empty: Story = {
  args: { tags: [] },
};

export const Typing: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("combobox"), "bi");
  },
};

export const Suggestions: Story = {
  args: {
    suggestions: ["backend", "billing", "frontend", "internal", "production"],
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("combobox"));
  },
};

export const SuggestionsWhileTyping: Story = {
  args: {
    suggestions: ["backend", "billing", "frontend", "internal", "production"],
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("combobox"), "bi");
  },
};

export const InvalidTag: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("combobox"), "two words");
  },
};

export const AtLimit: Story = {
  args: {
    tags: ["backend", "billing", "internal", "production", "team-a"],
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("combobox"), "more");
  },
};

export const Wrapped: Story = {
  args: {
    tags: ["backend", "billing", "internal", "production"],
  },
};

export const Disabled: Story = {
  args: { disabled: true },
};
