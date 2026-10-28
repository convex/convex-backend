import { Meta, StoryObj } from "@storybook/nextjs";
import { BackupRestoreOngoing } from "./BackupRestoreStatus";

const meta = {
  component: BackupRestoreOngoing,
} satisfies Meta<typeof BackupRestoreOngoing>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    progressMessage: 'Importing "_storage"',
  },
};
