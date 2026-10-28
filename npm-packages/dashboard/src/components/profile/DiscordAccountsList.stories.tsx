import { Meta, StoryObj } from "@storybook/nextjs";
import { DiscordAccountsList } from "./DiscordAccounts";
import { Sheet } from "@ui/Sheet";

const meta = {
  component: DiscordAccountsList,
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
} satisfies Meta<typeof DiscordAccountsList>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    accounts: [
      // With global name
      {
        id: "1108555035970904084",
        details: {
          username: "nicolas_convex",
          discriminator: "0",
          global_name: "Nicolas",
          avatar: "ee5e5b9486f5d7098669f6a53b13bae0",
        },
      },
      // Without global name
      {
        id: "689509994864836650",
        details: {
          username: "sujayakar",
          avatar: "278d8a0a2e21f1983bef08e2e0c73adb",
          global_name: null,
          discriminator: "0",
        },
      },
      // Without discriminator + without avatar
      {
        id: "1019379753142206575",
        details: {
          username: "presley4529",
          avatar: null,
          discriminator: "0",
          global_name: "presley",
        },
      },
      // With discriminator + without avatar
      {
        id: "1080664485565567056",
        details: {
          username: "Convex Indexer",
          avatar: null,
          discriminator: "7269",
          global_name: null,
        },
      },
      // Deleted account
      {
        id: "808537376864014593",
        details: null,
      },
    ],
  },
};
