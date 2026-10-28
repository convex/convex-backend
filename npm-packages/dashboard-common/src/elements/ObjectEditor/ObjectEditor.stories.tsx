import { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { ObjectEditor } from "@common/elements/ObjectEditor/ObjectEditor";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { fn } from "storybook/test";
import { Sheet } from "@ui/Sheet";

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.listById.default, ({ ids }) => ids.map(() => null))
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.getTableMapping.default, () => ({}));

const meta = {
  component: ObjectEditor,
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="h-64">
          <Sheet>
            <ObjectEditor {...args} />
          </Sheet>
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
} satisfies Meta<typeof ObjectEditor>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    defaultValue: null,
    onChange: fn(),
    onError: fn(),
    path: "document",
    mode: "editField",
  },
};
