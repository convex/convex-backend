import type { Meta, StoryObj } from "@storybook/nextjs";
import {
  BackfillingIndexes,
  IndexCreationStatus,
  SchemaValidationByTable,
  SchemaValidationStatus,
} from "./DeployProgress";

const schema: SchemaValidationByTable = {
  numDocsValidated: 489_400,
  totalDocs: 1_500_412,
  tables: [
    {
      tableName: "books",
      staged: false,
      state: "pending",
      numDocsValidated: 428_000,
      totalDocs: 1_190_000,
    },
    {
      tableName: "pages",
      staged: false,
      state: "valid",
      numDocsValidated: 400,
      totalDocs: 400,
    },
    {
      tableName: "authors",
      staged: false,
      state: "failed",
      error: "Document with ID abc123 does not match the schema",
      numDocsValidated: 0,
      totalDocs: 12,
    },
    {
      tableName: "reviews",
      staged: false,
      state: "pending",
      numDocsValidated: 0,
      totalDocs: 30_000,
    },
    {
      tableName: "shelves",
      staged: false,
      state: "pending",
      numDocsValidated: 2_100,
      totalDocs: 3_000,
    },
    {
      tableName: "tags",
      staged: true,
      state: "pending",
      numDocsValidated: 900,
      totalDocs: 9_000,
    },
    {
      tableName: "users",
      staged: false,
      state: "valid",
      numDocsValidated: 18_000,
      totalDocs: 18_000,
    },
    {
      tableName: "sessions",
      staged: false,
      state: "pending",
      numDocsValidated: 40_000,
      totalDocs: 250_000,
    },
  ],
};

const indexes: BackfillingIndexes = [
  {
    tableName: "books",
    name: "by_author",
    kind: "database",
    staged: false,
    stats: { numDocsIndexed: 380_000, totalDocs: 1_190_000 },
  },
  {
    tableName: "pages",
    name: "by_book_and_number",
    kind: "database",
    staged: true,
    stats: null,
  },
  {
    tableName: "pages",
    name: "search_body",
    kind: "search",
    staged: false,
    stats: { numDocsIndexed: 12_000, totalDocs: 88_000 },
  },
  {
    tableName: "reviews",
    name: "by_embedding",
    kind: "vector",
    staged: false,
    stats: { numDocsIndexed: 300, totalDocs: 5_400 },
  },
  {
    tableName: "reviews",
    name: "by_rating",
    kind: "database",
    staged: false,
    stats: { numDocsIndexed: 5_000, totalDocs: 5_400 },
  },
  {
    tableName: "shelves",
    name: "by_owner",
    kind: "database",
    staged: false,
    stats: null,
  },
  {
    tableName: "users",
    name: "by_email",
    kind: "database",
    staged: false,
    stats: { numDocsIndexed: 9_000, totalDocs: 18_000 },
  },
];

function DeployStatuses({
  schema,
  indexes,
  initiallyOpen,
}: {
  schema: SchemaValidationByTable | null;
  indexes: BackfillingIndexes;
  initiallyOpen?: boolean;
}) {
  return (
    <div className="flex w-fit flex-col gap-4 rounded-lg bg-background-secondary p-2 py-3">
      {schema && (
        <SchemaValidationStatus schema={schema} initiallyOpen={initiallyOpen} />
      )}
      {indexes.length > 0 && (
        <IndexCreationStatus indexes={indexes} initiallyOpen={initiallyOpen} />
      )}
    </div>
  );
}

const meta = {
  component: DeployStatuses,
} satisfies Meta<typeof DeployStatuses>;

export default meta;
type Story = StoryObj<typeof meta>;

// Hover or focus a line to unfold its detail; these stories start pinned
// open so the detail is visible without interaction.
export const SchemaAndIndexes: Story = {
  args: { schema, indexes, initiallyOpen: true },
};

export const Collapsed: Story = { args: { schema, indexes } };

export const SchemaOnly: Story = {
  args: { schema, indexes: [], initiallyOpen: true },
};

export const IndexesOnly: Story = {
  args: { schema: null, indexes, initiallyOpen: true },
};

export const UnknownTotals: Story = {
  args: {
    schema: {
      numDocsValidated: 250,
      totalDocs: null,
      tables: [
        {
          tableName: "books",
          staged: false,
          state: "pending",
          numDocsValidated: 250,
          totalDocs: null,
        },
      ],
    },
    indexes: [],
    initiallyOpen: true,
  },
};
