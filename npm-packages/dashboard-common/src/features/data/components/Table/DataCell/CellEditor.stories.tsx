import { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { fn } from "storybook/test";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { CellEditor } from "@common/features/data/components/Table/DataCell/CellEditor";

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.listById.default, ({ ids }) => ids.map(() => null))
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.getTableMapping.default, () => ({}));

const meta = {
  component: CellEditor,
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        {/* Approximates the popper the editor renders into: a column's width,
            with the editor sizing itself to its content. */}
        <div className="w-96">
          <CellEditor {...args} />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
  args: {
    onStopEditing: fn(),
    onSave: fn(),
  },
} satisfies Meta<typeof CellEditor>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    value: "hello@convex.dev",
  },
};

// A timestamp-like number adds the Ctrl+Shift+D hint to the shortcut bar.
export const Timestamp: Story = {
  args: {
    value: 1735689600000,
    inferIsDate: false,
  },
};
