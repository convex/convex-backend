import { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import React from "react";
import udfs from "@common/udfs";
import { EditDocumentPanel } from "@common/features/data/components/Table/EditDocumentPanel/EditDocumentPanel";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { Panel, PanelGroup } from "react-resizable-panels";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { fn } from "storybook/test";

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.listById.default, ({ ids }) => ids.map(() => null))
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.getTableMapping.default, () => ({}));

const meta = {
  component: EditDocumentPanel,
  args: {
    tableName: "users",
    onClose: fn(),
    onSave: fn(),
  },
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <PanelGroup direction="horizontal" className="fixed inset-0 size-full">
          <Panel />
          <EditDocumentPanel {...args} />
        </PanelGroup>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
} satisfies Meta<typeof EditDocumentPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Adding: Story = {
  args: { defaultDocument: {} },
};

export const Editing: Story = {
  args: { defaultDocument: { abc: 1, def: "ghi" } },
};
