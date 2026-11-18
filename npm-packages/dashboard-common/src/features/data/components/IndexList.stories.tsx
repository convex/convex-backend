import { Meta, StoryObj } from "@storybook/nextjs";
import { IndexesList as IndexList } from "@common/features/data/components/IndexList";

const meta = {
  component: IndexList,
  args: {
    tableName: "messages",
  },
  render: (args) => (
    <div className="h-screen overflow-y-auto bg-background-secondary p-4">
      <IndexList {...args} />
    </div>
  ),
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof IndexList>;

export default meta;
type Story = StoryObj<typeof meta>;

export const LoadingStory: Story = {
  args: { indexes: undefined },
};

export const EmptyState: Story = {
  args: {
    indexes: [],
  },
};

export const WithIndexes: Story = {
  args: {
    indexes: [
      {
        table: "my-table",
        name: "by_author",
        fields: ["author"],
        backfill: { state: "done" },
      },
      {
        table: "my-table",
        name: "by_channel_and_message",
        fields: ["channel", "message"],
        backfill: { state: "done" },
      },
      {
        table: "my-table",
        name: "search_index",
        fields: {
          searchField: "body",
          filterFields: ["channel", "author"],
        },
        backfill: { state: "done" },
      },
      {
        table: "my-table",
        name: "vector_index",
        fields: {
          vectorField: "body",
          filterFields: ["channel", "author"],
          dimensions: 1536,
        },
        backfill: { state: "done" },
      },
    ],
  },
};

export const WithUpdatingIndexes: Story = {
  args: {
    indexes: [
      {
        table: "my-table",
        name: "new_index_no_stats",
        fields: ["author"],
        backfill: { state: "backfilling" },
      },
      {
        table: "my-table",
        name: "new_index_missing_total",
        fields: ["author"],
        backfill: {
          state: "backfilling",
          stats: { numDocsIndexed: 100, totalDocs: null },
        },
      },
      {
        table: "my-table",
        name: "new_index_with_stats",
        fields: ["author"],
        backfill: {
          state: "backfilling",
          stats: { numDocsIndexed: 100, totalDocs: 1000 },
        },
      },
      {
        table: "my-table",
        name: "updated_index",
        fields: ["name"],
        backfill: { state: "done" },
      },
      {
        table: "my-table",
        name: "updated_index",
        fields: ["name"],
        staged: true,
        backfill: {
          state: "backfilling",
          stats: { numDocsIndexed: 500, totalDocs: 1000 },
        },
      },
      {
        table: "my-table",
        name: "updated_index",
        fields: ["name"],
        staged: true,
        backfill: {
          state: "backfilling",
          stats: { numDocsIndexed: 1000, totalDocs: 1000 },
        },
      },
      {
        table: "my-table",
        name: "updated_index",
        fields: ["name"],
        staged: true,
        backfill: {
          state: "backfilled",
          stats: { numDocsIndexed: 1000, totalDocs: 1000 },
        },
      },
      {
        table: "my-table",
        name: "updated_index",
        fields: ["name", "subtitle"],
        backfill: {
          state: "backfilling",
          stats: { numDocsIndexed: 500, totalDocs: 1000 },
        },
      },
      {
        table: "my-table",
        name: "updated_search_index",
        fields: { searchField: "title", filterFields: [] },
        backfill: { state: "done" },
      },
      {
        table: "my-table",
        name: "updated_search_index",
        fields: { searchField: "title", filterFields: ["author"] },
        backfill: {
          state: "backfilling",
          stats: { numDocsIndexed: 500, totalDocs: 1000 },
        },
      },
    ],
  },
};

export const WithComplexIndex: Story = {
  args: {
    indexes: [
      {
        table: "my-table",
        name: "by_channel_and_message_and_author_and_author_username_and_modification_date",
        fields: [
          "channel",
          "message",
          "author",
          "author_username",
          "modificationDate",
        ],
        backfill: {
          state: "backfilling",
          stats: { numDocsIndexed: 50, totalDocs: 100 },
        },
      },
    ],
  },
};

export const RecommendStagedIndex: Story = {
  args: {
    indexes: [
      {
        table: "my-table",
        name: "by_name",
        fields: ["name"],
        backfill: {
          state: "backfilling",
          stats: { numDocsIndexed: 5000, totalDocs: 100_000 },
        },
      },
    ],
  },
};
