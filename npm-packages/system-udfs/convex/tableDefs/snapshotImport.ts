import { defineTable } from "convex/server";
import { v } from "convex/values";

export const snapshotImportFormat = v.union(
  v.object({
    format: v.union(
      v.literal("csv"),
      v.literal("jsonl"),
      v.literal("json_array"),
    ),
    table: v.string(),
  }),
  v.object({
    format: v.literal("zip"),
  }),
);

export const snapshotImportMode = v.union(
  v.literal("RequireEmpty"),
  v.literal("Append"),
  v.literal("Replace"),
  v.literal("ReplaceAll"),
);

export const snapshotImportRequestor = v.union(
  v.object({
    type: v.literal("snapshotImport"),
  }),
  v.object({
    type: v.literal("cloudRestore"),
    sourceCloudBackupId: v.int64(),
  }),
);

export const snapshotImportsTable = defineTable({
  state: v.union(
    v.object({
      state: v.literal("uploaded"),
    }),
    v.object({
      state: v.literal("waiting_for_confirmation"),
      message_to_confirm: v.string(),
      require_manual_confirmation: v.boolean(),
    }),
    v.object({
      state: v.literal("in_progress"),
      progress_message: v.string(),
      checkpoint_messages: v.array(v.string()),
    }),
    v.object({
      state: v.literal("completed"),
      timestamp: v.int64(),
      num_rows_written: v.int64(),
    }),
    v.object({
      state: v.literal("failed"),
      error_message: v.string(),
    }),
  ),
  format: snapshotImportFormat,
  mode: snapshotImportMode,
  // Null for the app. Missing on documents written before components.
  component_path: v.optional(v.union(v.string(), v.null())),
  // Exactly one of these is set: older imports have `object_key`.
  object_key: v.optional(v.string()),
  fq_object_key: v.optional(v.string()),
  member_id: v.optional(v.union(v.int64(), v.null())),
  checkpoints: v.optional(
    v.union(
      v.null(),
      v.array(
        v.object({
          component_path: v.optional(v.union(v.string(), v.null())),
          display_table_name: v.string(),
          tablet_id: v.union(v.string(), v.null()),
          total_num_rows_to_write: v.int64(),
          num_rows_written: v.int64(),
          existing_rows_in_table: v.int64(),
          existing_rows_to_delete: v.int64(),
          is_missing_id_field: v.boolean(),
        }),
      ),
    ),
  ),
  requestor: snapshotImportRequestor,
});
