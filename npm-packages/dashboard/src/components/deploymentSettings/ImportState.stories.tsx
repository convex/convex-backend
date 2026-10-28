import { Meta, StoryObj } from "@storybook/nextjs";
import { Doc, Id } from "system-udfs/convex/_generated/dataModel";
import { ImportState } from "./SnapshotImport";
import { Sheet } from "@ui/Sheet";

const now = new Date();

const baseSnapshotImport: Doc<"_snapshot_imports"> & { memberName: string } = {
  _id: "abc123" as Id<"_snapshot_imports">,
  _creationTime: +now,
  member_id: BigInt(1),
  memberName: "John Doe",
  format: { format: "zip" },
  mode: "Replace",
  requestor: { type: "snapshotImport" },
  state: {
    state: "waiting_for_confirmation",
    message_to_confirm: "Please confirm this import",
    require_manual_confirmation: false,
  },
  checkpoints: [
    {
      component_path: null,
      display_table_name: "users",
      tablet_id: null,
      total_num_rows_to_write: BigInt(100),
      num_rows_written: BigInt(0),
      existing_rows_to_delete: BigInt(10),
      existing_rows_in_table: BigInt(50),
      is_missing_id_field: false,
    },
    {
      component_path: null,
      display_table_name: "posts",
      tablet_id: null,
      total_num_rows_to_write: BigInt(200),
      num_rows_written: BigInt(0),
      existing_rows_to_delete: BigInt(5),
      existing_rows_in_table: BigInt(20),
      is_missing_id_field: false,
    },
  ],
};

const meta = {
  component: ImportState,
  args: {
    snapshotImport: baseSnapshotImport,
  },
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof ImportState>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WaitingForConfirmation: Story = {};

export const WaitingForConfirmationWithMessage: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      checkpoints: null,
      state: {
        state: "waiting_for_confirmation",
        message_to_confirm:
          "This import will overwrite existing data. Please confirm.",
        require_manual_confirmation: true,
      },
    },
  },
};

export const Uploaded: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      state: {
        state: "uploaded",
      },
    },
  },
};

export const InProgress: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      state: {
        state: "in_progress",
        checkpoint_messages: ["Imported table users", "Imported table posts"],
        progress_message: "Importing table comments...",
      },
    },
  },
};

export const Completed: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      state: {
        state: "completed",
        timestamp: BigInt(Date.now() * 1_000_000),
        num_rows_written: BigInt(300),
      },
    },
  },
};

export const Failed: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      state: {
        state: "failed",
        error_message: "Import failed due to schema validation error",
      },
    },
  },
};

export const CSVFormat: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      format: { format: "csv", table: "users" },
    },
  },
};

export const JSONLFormat: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      format: { format: "jsonl", table: "users" },
    },
  },
};

export const JSONArrayFormat: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      format: { format: "json_array", table: "users" },
    },
  },
};

export const WithComponentPath: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      checkpoints: [
        {
          component_path: "myComponent",
          display_table_name: "users",
          tablet_id: null,
          total_num_rows_to_write: BigInt(100),
          num_rows_written: BigInt(0),
          existing_rows_to_delete: BigInt(10),
          existing_rows_in_table: BigInt(50),
          is_missing_id_field: false,
        },
        {
          component_path: "anotherComponent",
          display_table_name: "posts",
          tablet_id: null,
          total_num_rows_to_write: BigInt(200),
          num_rows_written: BigInt(0),
          existing_rows_to_delete: BigInt(5),
          existing_rows_in_table: BigInt(20),
          is_missing_id_field: false,
        },
      ],
    },
  },
};

export const WithStorageTable: Story = {
  args: {
    snapshotImport: {
      ...baseSnapshotImport,
      checkpoints: [
        {
          component_path: null,
          display_table_name: "_storage",
          tablet_id: null,
          total_num_rows_to_write: BigInt(1),
          num_rows_written: BigInt(0),
          existing_rows_to_delete: BigInt(0),
          existing_rows_in_table: BigInt(10),
          is_missing_id_field: false,
        },
        {
          component_path: null,
          display_table_name: "_storage",
          tablet_id: null,
          total_num_rows_to_write: BigInt(5),
          num_rows_written: BigInt(0),
          existing_rows_to_delete: BigInt(2),
          existing_rows_in_table: BigInt(10),
          is_missing_id_field: false,
        },
      ],
    },
  },
};
