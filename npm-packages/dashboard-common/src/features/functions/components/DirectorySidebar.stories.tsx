import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { Module } from "system-udfs/convex/_system/frontend/common";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { FunctionsProvider } from "@common/lib/functions/FunctionsProvider";
import { DirectorySidebar } from "./DirectorySidebar";

function rootModules(files: string[], functionsPerFile: number) {
  const modules: [string, Module][] = files.map((file) => [
    file,
    {
      functions: Array.from({ length: functionsPerFile }, (_, fn) => ({
        name: `fn${fn}`,
        lineno: fn + 1,
        udfType: "Query" as const,
        visibility: { kind: "public" as const },
      })),
      sourcePackageId: "sourcePackage",
    },
  ]);
  return [[null, modules]] as [null, [string, Module][]][];
}

function withDeployment(modules: ReturnType<typeof rootModules>) {
  const client = mockConvexReactClient()
    .registerQueryFake(udfs.components.list, () => [])
    .registerQueryFake(udfs.modules.listForAllComponents, () => modules);
  return () => (
    <ConvexProvider client={client}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <FunctionsProvider>
          <DirectorySidebar onChangeFunction={() => {}} />
        </FunctionsProvider>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  );
}

const meta = {
  component: DirectorySidebar,
  args: { onChangeFunction: () => {} },
  // The functions page shows the sidebar in a resizable panel of about this
  // width.
  decorators: [
    (Story) => (
      <div className="h-screen w-72">
        <Story />
      </div>
    ),
  ],
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof DirectorySidebar>;

export default meta;
type Story = StoryObj<typeof meta>;

// Small enough that every root-level file starts expanded.
export const SmallDeployment: Story = {
  render: withDeployment(
    rootModules(["messages.js", "users.js", "channels.js"], 4),
  ),
};

// 1,000 root-level files of 5 functions each: expanding all of them would mount
// 6,000 rows, so they start collapsed.
export const LargeFlatDeployment: Story = {
  render: withDeployment(
    rootModules(
      Array.from({ length: 1000 }, (_, i) => `file${i}.js`),
      5,
    ),
  ),
};
