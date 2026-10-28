import { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { ValidatorJSON } from "convex/values";
import { Shape } from "shapes";
import { mocked } from "storybook/test";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { useTableShapes } from "@common/lib/deploymentApi";
import { SchemaJson } from "@common/lib/format";
import { SchemaView } from "@common/features/schema/components/SchemaView";

// A schema with three tables, id() references between them, and one of each
// index kind (database, search, vector) so the graph and side panel exercise
// every code path.
const sampleSchema: SchemaJson = {
  schemaValidation: true,
  tables: [
    {
      tableName: "channels",
      documentType: {
        type: "object",
        value: {
          name: { fieldType: { type: "string" }, optional: false },
          description: { fieldType: { type: "string" }, optional: true },
        },
      },
      indexes: [{ indexDescriptor: "by_name", fields: ["name"] }],
      searchIndexes: [],
      vectorIndexes: [],
    },
    {
      tableName: "messages",
      documentType: {
        type: "object",
        value: {
          body: { fieldType: { type: "string" }, optional: false },
          author: {
            fieldType: { type: "id", tableName: "users" },
            optional: false,
          },
          channel: {
            fieldType: { type: "id", tableName: "channels" },
            optional: false,
          },
          embedding: {
            fieldType: { type: "array", value: { type: "number" } },
            optional: true,
          },
        },
      },
      indexes: [{ indexDescriptor: "by_channel", fields: ["channel"] }],
      searchIndexes: [
        {
          indexDescriptor: "search_body",
          searchField: "body",
          filterFields: ["channel"],
        },
      ],
      vectorIndexes: [
        {
          indexDescriptor: "by_embedding",
          vectorField: "embedding",
          dimensions: 1536,
          filterFields: [],
        },
      ],
    },
    {
      tableName: "users",
      documentType: {
        type: "object",
        value: {
          name: { fieldType: { type: "string" }, optional: false },
          email: { fieldType: { type: "string" }, optional: false },
          bestFriend: {
            fieldType: { type: "id", tableName: "users" },
            optional: true,
          },
          // A nested object — collapses to `{ … }`, expands to the full shape.
          preferences: {
            fieldType: {
              type: "object",
              value: {
                theme: {
                  fieldType: {
                    type: "union",
                    value: [
                      { type: "literal", value: "light" },
                      { type: "literal", value: "dark" },
                    ],
                  },
                  optional: false,
                },
                notifications: {
                  fieldType: { type: "boolean" },
                  optional: false,
                },
              },
            },
            optional: true,
          },
          // A union of objects — collapses to `{ … } | { … }`.
          contact: {
            fieldType: {
              type: "union",
              value: [
                {
                  type: "object",
                  value: {
                    kind: {
                      fieldType: { type: "literal", value: "email" },
                      optional: false,
                    },
                    address: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                  },
                },
                {
                  type: "object",
                  value: {
                    kind: {
                      fieldType: { type: "literal", value: "phone" },
                      optional: false,
                    },
                    number: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                  },
                },
              ],
            },
            optional: true,
          },
        },
      },
      indexes: [{ indexDescriptor: "by_email", fields: ["email"] }],
      searchIndexes: [],
      vectorIndexes: [],
    },
  ],
};

// Data shapes inferred for the same tables, used by the "inferred from shapes"
// story (no saved schema) and to satisfy `useTableShapes` everywhere else.
function objectShape(
  fields: { name: string; optional?: boolean; shape: Shape }[],
): Shape {
  return {
    type: "Object",
    fields: fields.map((f) => ({
      fieldName: f.name,
      optional: f.optional ?? false,
      shape: f.shape,
    })),
  };
}

const sampleShapes = new Map<string, Shape>([
  [
    "channels",
    objectShape([
      { name: "_id", shape: { type: "Id", tableName: "channels" } },
      { name: "_creationTime", shape: { type: "Float64", float64Range: {} } },
      { name: "name", shape: { type: "String" } },
      { name: "description", optional: true, shape: { type: "String" } },
    ]),
  ],
  [
    "messages",
    objectShape([
      { name: "_id", shape: { type: "Id", tableName: "messages" } },
      { name: "_creationTime", shape: { type: "Float64", float64Range: {} } },
      { name: "body", shape: { type: "String" } },
      { name: "author", shape: { type: "Id", tableName: "users" } },
      { name: "channel", shape: { type: "Id", tableName: "channels" } },
    ]),
  ],
  [
    "users",
    objectShape([
      { name: "_id", shape: { type: "Id", tableName: "users" } },
      { name: "_creationTime", shape: { type: "Float64", float64Range: {} } },
      { name: "name", shape: { type: "String" } },
      { name: "email", shape: { type: "String" } },
    ]),
  ],
]);

// The same data shapes plus two tables that aren't declared in the schema:
// `auditLogs` (which references a schema table by id) and `featureFlags`. With a
// saved schema present, these are merged into the diagram and marked with a `*`.
const shapesWithExtraTables = new Map<string, Shape>([
  ...sampleShapes,
  [
    "auditLogs",
    objectShape([
      { name: "_id", shape: { type: "Id", tableName: "auditLogs" } },
      { name: "_creationTime", shape: { type: "Float64", float64Range: {} } },
      { name: "action", shape: { type: "String" } },
      { name: "actor", shape: { type: "Id", tableName: "users" } },
    ]),
  ],
  [
    "featureFlags",
    objectShape([
      { name: "_id", shape: { type: "Id", tableName: "featureFlags" } },
      { name: "_creationTime", shape: { type: "Float64", float64Range: {} } },
      { name: "key", shape: { type: "String" } },
      { name: "enabled", shape: { type: "Boolean" } },
    ]),
  ],
]);

function makeClient({
  active,
  inProgress,
}: {
  active?: SchemaJson;
  inProgress?: SchemaJson;
} = {}) {
  return (
    mockConvexReactClient()
      .registerQueryFake(udfs.components.list, () => [])
      .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
      .registerQueryFake(udfs.getSchemas.default, () => ({
        active: active ? JSON.stringify(active) : undefined,
        inProgress: inProgress ? JSON.stringify(inProgress) : undefined,
      }))
      .registerQueryFake(udfs.getSchemas.schemaValidationProgress, () => null)
      // The side panel reuses the data page's IndexList, which queries indexes.
      .registerQueryFake(udfs.indexes.default, () => [])
  );
}

const deploymentInfo = {
  ...mockDeploymentInfo,
  deploymentsURI: "/t/acme/my-amazing-app/happy-capybara-123",
  projectsURI: "/t/acme/my-amazing-app",
  teamsURI: "/t/acme",
};

const meta = {
  component: SchemaView,
  args: { clustering: false },
  argTypes: {
    // Toggle table grouping live to compare grouped vs. ungrouped layouts.
    clustering: { control: "boolean" },
  },
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/schema",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "happy-capybara-123",
        },
      },
    },
    a11y: { test: "todo" },
  },
  decorators: [
    (Story) => (
      <div className="h-screen">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof SchemaView>;

export default meta;
type Story = StoryObj<typeof meta>;

/** A saved schema: the graph and indexes come from `convex/schema.ts`. */
export const SavedSchema: Story = {
  decorators: [
    (Story) => {
      mocked(useTableShapes).mockReturnValue({
        tables: sampleShapes,
        hadError: false,
      });
      return (
        <ConvexProvider client={makeClient({ active: sampleSchema })}>
          <DeploymentInfoContext.Provider value={deploymentInfo}>
            <Story />
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      );
    },
  ],
};

/**
 * A saved schema alongside tables that exist in the data but aren't declared in
 * it. The undeclared tables (`auditLogs`, `featureFlags`) are merged into the
 * diagram with their fields inferred from shapes and marked with a `*`.
 */
export const SchemaWithTablesNotInSchema: Story = {
  decorators: [
    (Story) => {
      mocked(useTableShapes).mockReturnValue({
        tables: shapesWithExtraTables,
        hadError: false,
      });
      return (
        <ConvexProvider client={makeClient({ active: sampleSchema })}>
          <DeploymentInfoContext.Provider value={deploymentInfo}>
            <Story />
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      );
    },
  ],
};

/** No saved schema: tables and relationships are inferred from the data. */
export const InferredFromShapes: Story = {
  decorators: [
    (Story) => {
      mocked(useTableShapes).mockReturnValue({
        tables: sampleShapes,
        hadError: false,
      });
      return (
        <ConvexProvider client={makeClient()}>
          <DeploymentInfoContext.Provider value={deploymentInfo}>
            <Story />
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      );
    },
  ],
};

/** No tables yet — the empty state with a call to add a schema. */
export const Empty: Story = {
  decorators: [
    (Story) => {
      mocked(useTableShapes).mockReturnValue({
        tables: new Map(),
        hadError: false,
      });
      return (
        <ConvexProvider client={makeClient()}>
          <DeploymentInfoContext.Provider value={deploymentInfo}>
            <Story />
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      );
    },
  ],
};

/** Shapes failed to load. */
export const LoadError: Story = {
  decorators: [
    (Story) => {
      mocked(useTableShapes).mockReturnValue({
        tables: new Map(),
        hadError: true,
      });
      return (
        <ConvexProvider client={makeClient()}>
          <DeploymentInfoContext.Provider value={deploymentInfo}>
            <Story />
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      );
    },
  ],
};

/** Still loading — shapes haven't resolved yet. */
export const Loading: Story = {
  decorators: [
    (Story) => {
      mocked(useTableShapes).mockReturnValue({
        tables: undefined,
        hadError: false,
      });
      return (
        <ConvexProvider client={makeClient({ active: sampleSchema })}>
          <DeploymentInfoContext.Provider value={deploymentInfo}>
            <Story />
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      );
    },
  ],
};

/** The viewer lacks data-view permission. */
export const NoPermission: Story = {
  decorators: [
    (Story) => {
      // SchemaView reads shapes before the permission gate, so the hook still
      // runs here — stub it (the story has no connected deployment to fetch
      // from, which would otherwise throw).
      mocked(useTableShapes).mockReturnValue({
        tables: new Map(),
        hadError: false,
      });
      return (
        <ConvexProvider client={makeClient()}>
          <DeploymentInfoContext.Provider value={deploymentInfo}>
            <PermissionsContext.Provider
              value={{
                canViewDataCached: false,
                useIsOperationAllowed: () => false,
              }}
            >
              <Story />
            </PermissionsContext.Provider>
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      );
    },
  ],
};

// --- Edge-case stories ------------------------------------------------------
//
// Compact builders for saved-schema document types, so the schemas below stay
// readable. `id`/`array`/`record`/`union`/`obj` build validator JSON; `table`
// wraps an object document type; `savedSchemaStory` wires it into the standard
// decorators (shapes are stubbed empty — these render from the saved schema).

type TableJson = SchemaJson["tables"][number];

// A field is either a bare validator or a validator wrapped with `optional`. A
// bare `ValidatorJSON` always has a string `type`; the wrapped form's `type` is
// itself a validator (an object), which is how `normalizeField` tells them
// apart.
type FieldSpec = { type: ValidatorJSON; optional?: boolean };
type Field = ValidatorJSON | FieldSpec;

const str: ValidatorJSON = { type: "string" };
const num: ValidatorJSON = { type: "number" };
const id = (tableName: string): ValidatorJSON => ({ type: "id", tableName });
const array = (value: ValidatorJSON): ValidatorJSON => ({
  type: "array",
  value,
});
const record = (values: ValidatorJSON): ValidatorJSON => ({
  type: "record",
  keys: { type: "string" },
  values: { fieldType: values, optional: false },
});
const union = (...value: ValidatorJSON[]): ValidatorJSON => ({
  type: "union",
  value,
});

function normalizeField(field: Field): {
  fieldType: ValidatorJSON;
  optional: boolean;
} {
  if (typeof (field as FieldSpec).type === "object") {
    const spec = field as FieldSpec;
    return { fieldType: spec.type, optional: spec.optional ?? false };
  }
  return { fieldType: field as ValidatorJSON, optional: false };
}

function obj(fields: Record<string, Field>): ValidatorJSON {
  return {
    type: "object",
    value: Object.fromEntries(
      Object.entries(fields).map(([name, field]) => [
        name,
        normalizeField(field),
      ]),
    ),
  };
}

function table(
  tableName: string,
  fields: Record<string, Field>,
  extras: Partial<Omit<TableJson, "tableName" | "documentType">> = {},
): TableJson {
  return {
    tableName,
    documentType: obj(fields),
    indexes: [],
    searchIndexes: [],
    vectorIndexes: [],
    ...extras,
  };
}

function savedSchemaStory(tables: TableJson[]): Story {
  const active: SchemaJson = { schemaValidation: true, tables };
  return {
    decorators: [
      (Story) => {
        mocked(useTableShapes).mockReturnValue({
          tables: new Map(),
          hadError: false,
        });
        return (
          <ConvexProvider client={makeClient({ active })}>
            <DeploymentInfoContext.Provider value={deploymentInfo}>
              <Story />
            </DeploymentInfoContext.Provider>
          </ConvexProvider>
        );
      },
    ],
  };
}

/**
 * Every shape of `v.id(...)` reference: direct, optional self-reference, nested
 * in an array, in a record's values, a union over two id types (one field, two
 * edges), nested inside an object inside an array, and a dangling reference to a
 * table that isn't in the schema (shown on the field, but no edge drawn).
 */
export const ReferenceVarieties: Story = savedSchemaStory([
  table("users", {
    name: str,
    // Optional self-reference — a self-edge.
    manager: { type: id("users"), optional: true },
  }),
  table("teams", {
    name: str,
    lead: { type: id("users") },
    // Reference nested in an array — `Id<users>[]`.
    members: { type: array(id("users")) },
  }),
  table("posts", {
    title: str,
    // Direct reference.
    author: { type: id("users") },
    // References as a record's values — `Record<string, Id<users>>`.
    reactions: { type: record(id("users")) },
    // A union over two id types — one field, two outgoing edges.
    owner: { type: union(id("users"), id("teams")) },
    // A reference nested inside an object inside an array.
    attachments: { type: array(obj({ uploadedBy: { type: id("users") } })) },
    // A reference to a table that no longer exists: shown on the field, but no
    // edge is drawn because the target isn't in the schema.
    legacyOwner: { type: id("deletedTable"), optional: true },
  }),
]);

/** A reference cycle (a → b → c → a) — exercises cyclic layout. */
export const CyclicReferences: Story = savedSchemaStory([
  table("a", { label: str, next: { type: id("b") } }),
  table("b", { label: str, next: { type: id("c") } }),
  table("c", { label: str, next: { type: id("a") } }),
]);

/** A single isolated table — no edges to lay out. */
export const SingleTable: Story = savedSchemaStory([
  table("singleton", { value: str, updatedAt: num }),
]);

/**
 * A table whose document type is an empty object renders a "no fields" row in
 * the node and the empty-fields message in the side panel. `owners` references
 * it so it's still drawn.
 */
export const TableWithNoFields: Story = savedSchemaStory([
  table("flags", {}),
  table("owners", { name: str, flag: { type: id("flags") } }),
]);

/**
 * One of every scalar and compound type, and more fields than fit in a node, so
 * the node collapses extras into a "+N more fields" row while the side panel
 * lists them all.
 */
export const WideTable: Story = savedSchemaStory([
  table("everything", {
    aString: str,
    aNumber: num,
    aBoolean: { type: { type: "boolean" } },
    aBigint: { type: { type: "bigint" } },
    aNull: { type: { type: "null" } },
    aBytes: { type: { type: "bytes" } },
    anAny: { type: { type: "any" } },
    aStringLiteral: { type: { type: "literal", value: "active" } },
    aNumberLiteral: { type: { type: "literal", value: 42 } },
    aStringArray: { type: array(str) },
    aRecord: { type: record(num) },
    aNestedObject: { type: obj({ lat: { type: num }, lng: { type: num } }) },
    aUnion: { type: union(str, num, { type: "null" }) },
    anOptional: { type: str, optional: true },
    aSelfRef: { type: id("everything"), optional: true },
    extra1: { type: str },
    extra2: { type: str },
    extra3: { type: str },
    extra4: { type: str },
  }),
]);

/**
 * A table with all three index kinds and more indexes than fit, so the node
 * collapses extras into a "+N more indexes" row.
 */
export const ManyIndexes: Story = savedSchemaStory([
  table("users", { name: str }),
  table(
    "events",
    {
      name: str,
      userId: { type: id("users") },
      body: str,
      createdAt: num,
      embedding: { type: array(num), optional: true },
    },
    {
      indexes: [
        { indexDescriptor: "by_user", fields: ["userId"] },
        { indexDescriptor: "by_created", fields: ["createdAt"] },
        {
          indexDescriptor: "by_user_and_created",
          fields: ["userId", "createdAt"],
        },
        { indexDescriptor: "by_name", fields: ["name"] },
        { indexDescriptor: "by_name_and_user", fields: ["name", "userId"] },
        {
          indexDescriptor: "by_everything",
          fields: ["name", "userId", "createdAt"],
        },
      ],
      searchIndexes: [
        {
          indexDescriptor: "search_body",
          searchField: "body",
          filterFields: ["userId"],
        },
      ],
      vectorIndexes: [
        {
          indexDescriptor: "by_embedding",
          vectorField: "embedding",
          dimensions: 1536,
          filterFields: [],
        },
      ],
    },
  ),
]);

export const UnionDocumentType: Story = savedSchemaStory([
  table("users", { name: str }),
  {
    tableName: "events",
    documentType: union(
      obj({
        kind: { type: { type: "literal", value: "click" } },
        actor: { type: id("users") },
        x: { type: num },
        y: { type: num },
      }),
      obj({
        kind: { type: { type: "literal", value: "pageview" } },
        actor: { type: id("users") },
        url: { type: str },
      }),
    ),
    indexes: [],
    searchIndexes: [],
    vectorIndexes: [],
  },
]);

export const UnionDocumentTypeNoDiscriminator: Story = savedSchemaStory([
  {
    tableName: "payloads",
    documentType: union(
      obj({
        text: { type: str },
        length: { type: num },
      }),
      obj({
        blob: { type: { type: "bytes" } },
        size: { type: num },
      }),
    ),
    indexes: [],
    searchIndexes: [],
    vectorIndexes: [],
  },
]);

const inferredUnionShapes = new Map<string, Shape>([
  [
    "events",
    {
      type: "Union",
      shapes: [
        objectShape([
          { name: "_id", shape: { type: "Id", tableName: "events" } },
          {
            name: "_creationTime",
            shape: { type: "Float64", float64Range: {} },
          },
          { name: "name", shape: { type: "String" } },
          { name: "count", shape: { type: "Float64", float64Range: {} } },
        ]),
        { type: "String" },
      ],
    } as Shape,
  ],
]);

export const InferredUnionShape: Story = {
  decorators: [
    (Story) => {
      mocked(useTableShapes).mockReturnValue({
        tables: inferredUnionShapes,
        hadError: false,
      });
      return (
        <ConvexProvider client={makeClient()}>
          <DeploymentInfoContext.Provider value={deploymentInfo}>
            <Story />
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      );
    },
  ],
};

/**
 * One table referenced by many others (a fan-in star) — exercises a high-fan-in
 * layout.
 */
export const HubTable: Story = savedSchemaStory([
  table("users", { name: str }),
  ...Array.from({ length: 8 }, (_, i) =>
    table(`dependent_${i}`, {
      label: str,
      owner: { type: id("users") },
    }),
  ),
]);

/**
 * A large schema (~300 tables, 10 fields each). The tables form `WIDTH`
 * parallel chains off a single root, so ELK's layered layout gives ~`WIDTH`
 * nodes per level — a roughly rectangular graph rather than the very wide, short
 * one a bushy tree would produce. Each node references its parent.
 */
const manyTablesSchema = (() => {
  const COUNT = 300;
  const WIDTH = 18;
  const FIELD_COUNT = 10;
  const name = (n: number) => `table_${String(n).padStart(3, "0")}`;
  return Array.from({ length: COUNT }, (_, i) => {
    const fields: Record<string, Field> = { name: str };
    if (i > 0) {
      // First WIDTH nodes hang off the root; each later node extends the chain
      // WIDTH slots above it.
      const parent = i <= WIDTH ? 0 : i - WIDTH;
      fields.parent = { type: id(name(parent)) };
    }
    // Pad to FIELD_COUNT fields with assorted scalars (no extra edges).
    let f = 1;
    while (Object.keys(fields).length < FIELD_COUNT) {
      fields[`field${f}`] = f % 2 === 0 ? num : str;
      f += 1;
    }
    return table(name(i), fields);
  });
})();

export const ManyTables: Story = savedSchemaStory(manyTablesSchema);

/**
 * The same large schema with clustering on: one giant connected component, so
 * connected-component grouping does nothing and community detection carries
 * the load — it splits the tree of chains into contiguous sub-groups, each
 * laid out together and drawn inside a labelled hull. Compare against
 * `ManyTables` to judge grouped vs. ungrouped layout on a large graph.
 */
export const ManyTablesClustered: Story = {
  ...savedSchemaStory(manyTablesSchema),
  args: { clustering: true },
};

/**
 * Several disconnected islands plus two fully isolated tables, clustering on.
 * Each island of 2+ tables becomes its own labelled group; the isolated tables
 * (`settings`, `migrations`) stay ungrouped. Exercises the connected-component
 * half of the grouping.
 */
export const DisconnectedComponents: Story = {
  ...savedSchemaStory([
    // Island 1: a blog-ish cluster around `users`.
    table("users", { name: str }),
    table("posts", { title: str, author: { type: id("users") } }),
    table("comments", {
      body: str,
      post: { type: id("posts") },
      author: { type: id("users") },
    }),
    // Island 2: a commerce cluster around `orders`.
    table("products", { name: str, price: num }),
    table("orders", { total: num }),
    table("orderItems", {
      order: { type: id("orders") },
      product: { type: id("products") },
      quantity: num,
    }),
    // Island 3: a mutual pair.
    table("teams", { name: str, owner: { type: id("teamMembers") } }),
    table("teamMembers", { name: str, team: { type: id("teams") } }),
    // Isolated tables — left ungrouped.
    table("settings", { key: str, value: str }),
    table("migrations", { version: num, appliedAt: num }),
  ]),
  args: { clustering: true },
};

/**
 * One connected component that community detection splits into three
 * internally-dense groups linked by a few cross-group references. Those
 * cross-group edges "escape" their clusters and are drawn crossing the hull
 * boundaries — the case where grouping is a heuristic, not a hard partition.
 */
export const ConnectedClusters: Story = {
  ...savedSchemaStory([
    // Commerce group (dense internally), with edges out to catalog and users.
    table("orders", { user: { type: id("users") }, total: num }),
    table("orderItems", {
      order: { type: id("orders") },
      product: { type: id("products") },
      quantity: num,
    }),
    table("payments", { order: { type: id("orders") }, amount: num }),
    table("shipments", {
      order: { type: id("orders") },
      address: { type: id("addresses") },
    }),
    table("refunds", { payment: { type: id("payments") }, amount: num }),
    // Catalog group.
    table("products", {
      category: { type: id("categories") },
      supplier: { type: id("suppliers") },
      name: str,
    }),
    table("categories", { name: str }),
    table("variants", { product: { type: id("products") }, sku: str }),
    table("inventory", { product: { type: id("products") }, count: num }),
    table("suppliers", { name: str }),
    // Users group (star around `users`).
    table("users", { name: str }),
    table("profiles", { user: { type: id("users") }, bio: str }),
    table("sessions", { user: { type: id("users") }, token: str }),
    table("addresses", { user: { type: id("users") }, line1: str }),
    table("preferences", { user: { type: id("users") }, theme: str }),
  ]),
  args: { clustering: true },
};

/**
 * A more tangled schema: loose content / social / commerce groups, but with
 * heavy cross-linking (almost everything references `users`, and tables
 * reference across groups freely). Community detection has to make judgement
 * calls here, so this is the story for eyeballing how sensible — or arbitrary —
 * the groupings look when the structure is genuinely muddy.
 */
export const TangledClusters: Story = {
  ...savedSchemaStory([
    table("users", {
      name: str,
      invitedBy: { type: id("users"), optional: true },
    }),
    table("posts", {
      author: { type: id("users") },
      product: { type: id("products"), optional: true },
    }),
    table("comments", {
      author: { type: id("users") },
      post: { type: id("posts") },
      replyTo: { type: id("comments"), optional: true },
    }),
    table("reactions", {
      user: { type: id("users") },
      post: { type: id("posts"), optional: true },
      comment: { type: id("comments"), optional: true },
    }),
    table("tags", { name: str, createdBy: { type: id("users") } }),
    table("postTags", {
      post: { type: id("posts") },
      tag: { type: id("tags") },
    }),
    table("follows", {
      follower: { type: id("users") },
      following: { type: id("users") },
    }),
    table("messages", {
      from: { type: id("users") },
      to: { type: id("users") },
      aboutPost: { type: id("posts"), optional: true },
    }),
    table("notifications", {
      user: { type: id("users") },
      post: { type: id("posts"), optional: true },
      comment: { type: id("comments"), optional: true },
      message: { type: id("messages"), optional: true },
    }),
    table("products", {
      seller: { type: id("users") },
      featuredPost: { type: id("posts"), optional: true },
    }),
    table("orders", {
      buyer: { type: id("users") },
      product: { type: id("products") },
    }),
    table("reviews", {
      author: { type: id("users") },
      product: { type: id("products") },
      order: { type: id("orders") },
    }),
    table("carts", {
      user: { type: id("users") },
      product: { type: id("products") },
    }),
  ]),
  args: { clustering: true },
};
