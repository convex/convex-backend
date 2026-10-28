import { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { useMemo, useState } from "react";
import { FilterExpression } from "system-udfs/convex/_system/frontend/lib/filters";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import {
  ConnectedDeployment,
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { fn } from "storybook/test";
import { Index } from "@common/features/data/lib/api";
import { IndexFilterBar } from "./IndexFilterBar";
import { buildIndexDefs } from "./filterModel";
import { useFilterActions } from "./useFilterActions";

// @ts-expect-error -- simplified mock for Storybook
const deployment: ConnectedDeployment = {};

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.listById.default, ({ ids }) => ids.map(() => null))
  .registerQueryFake(udfs.getVersion.default, () => "0.19.0")
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.getTableMapping.default, () => ({}));

const indexes: Index[] = [
  {
    name: "by_team",
    fields: ["team", "_creationTime"],
    backfill: { state: "done" },
  },
  {
    name: "by_team_status",
    fields: ["team", "status", "_creationTime"],
    backfill: { state: "done" },
  },
  {
    name: "search_title",
    fields: { searchField: "title", filterFields: ["team", "status"] },
    backfill: { state: "done" },
  },
];

const tableFields = [
  "_id",
  "_creationTime",
  "team",
  "status",
  "title",
  "score",
];
const defaultDocument = {
  _id: "j57bynpqhgjdjcfm2dxpj3j7vx774s1h",
  _creationTime: 1700000000000,
  team: "eng",
  status: "active",
  title: "Hello",
  score: 42,
};

function Example({ initialFilters }: { initialFilters?: FilterExpression }) {
  const connectedDeployment = useMemo(
    () => ({ deployment, isDisconnected: false }),
    [],
  );
  return (
    <ConnectedDeploymentContext.Provider value={connectedDeployment}>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
          <ExampleInner initialFilters={initialFilters} />
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  );
}

function ExampleInner({
  initialFilters,
}: {
  initialFilters?: FilterExpression;
}) {
  const [filters, setFilters] = useState<FilterExpression | undefined>(
    initialFilters,
  );
  const [draftFilters, setDraftFilters] = useState<
    FilterExpression | undefined
  >(initialFilters);
  const indexDefs = useMemo(() => buildIndexDefs(indexes), []);
  const actions = useFilterActions({
    filters,
    draftFilters,
    setDraftFilters,
    applyFilters: (next) => {
      setFilters(next);
      setDraftFilters(next);
    },
    indexDefs,
    defaultDocument,
  });
  return (
    <div className="flex flex-col gap-4">
      <IndexFilterBar
        actions={actions}
        indexDefs={indexDefs}
        tableName="tasks"
        tableFields={tableFields}
        defaultDocument={defaultDocument}
        activeSchema={null}
        numRows={1234}
        numRowsLoaded={50}
        hasFilters={filters !== undefined}
        allFields={["*select", ...tableFields]}
        hiddenColumns={[]}
        setHiddenColumns={fn()}
        columnOrder={[]}
        setColumnOrder={fn()}
      />
      <pre className="text-xs text-content-secondary">
        {JSON.stringify(filters, null, 2)}
      </pre>
    </div>
  );
}

const meta = {
  component: Example,
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof Example>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = { args: {} };

export const IndexedPath: Story = {
  args: {
    initialFilters: {
      clauses: [],
      order: "desc",
      index: {
        name: "by_team_status",
        clauses: [
          { type: "indexEq", enabled: true, value: "eng" },
          { type: "indexEq", enabled: true, value: "active" },
        ],
      },
    },
  },
};

export const RangeEndsThePath: Story = {
  args: {
    initialFilters: {
      clauses: [],
      order: "desc",
      index: {
        name: "by_team",
        clauses: [
          { type: "indexEq", enabled: true, value: "eng" },
          {
            type: "indexRange",
            enabled: true,
            lowerOp: "gte",
            lowerValue: 1700000000000,
          },
        ],
      },
    },
  },
};

export const WithScanFilter: Story = {
  args: {
    initialFilters: {
      clauses: [
        { id: "a", field: "score", op: "gt", value: 10, enabled: true },
      ],
      order: "desc",
      index: {
        name: "by_team",
        clauses: [{ type: "indexEq", enabled: true, value: "eng" }],
      },
    },
  },
};

export const Search: Story = {
  args: {
    initialFilters: {
      clauses: [],
      order: "asc",
      index: {
        name: "search_title",
        search: "hello",
        clauses: [{ field: "team", enabled: true, value: "eng" }],
      },
    },
  },
};
