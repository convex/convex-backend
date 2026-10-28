import { Base64, v } from "convex/values";
import { queryPrivateSystem } from "../secretSystemTables";
import { decodeId, encodeId } from "id-encoding";
import { DataModel, Doc, Id } from "../../_generated/dataModel";
import { GenericDatabaseReader } from "convex/server";

async function getTableId(
  db: GenericDatabaseReader<DataModel>,
  tableName: string,
  tableNamespace: string | null,
): Promise<string | undefined> {
  // Get the table id for the tablename
  const tablesWithName = await db
    .query("_tables")
    .withIndex("by_name", (q) => q.eq("name", tableName))
    // eslint-disable-next-line @convex-dev/no-filter-in-query -- FIXME: we could have a `_by_name_and_state` index here (but we’re already using a filter for name so it’s still okay)
    .filter((q) => q.eq(q.field("state"), "active"))
    .collect();
  let tableId;
  if (tableNamespace === null) {
    const tables = tablesWithName.filter(
      (table) => table.namespace === undefined,
    );
    if (tables.length !== 1) {
      return undefined;
    }
    tableId = tables[0]._id;
  } else {
    const tables = tablesWithName.filter(
      (table) => table.namespace && table.namespace.id === tableNamespace,
    );
    if (tables.length !== 1) {
      return undefined;
    }
    tableId = tables[0]._id;
  }
  const decodedId = decodeId(tableId);
  const tableInternalId = decodedId.internalId;
  const urlSafeInternalId =
    Base64.fromByteArrayUrlSafeNoPadding(tableInternalId);
  return urlSafeInternalId;
}

type IndexConfig = Doc<"_index">["config"];

function indexFieldsAndState(config: IndexConfig): {
  fields:
    | string[]
    | { searchField: string; filterFields: string[] }
    | {
        vectorField: string;
        filterFields: string[];
        dimensions: number;
      };
  state: "backfilling" | "backfilled" | "done";
  staged: boolean;
} {
  switch (config.type) {
    case "database": {
      const stateType = config.onDiskState.type;
      let staged;
      let state;
      switch (stateType) {
        case "Backfilling":
          staged = config.onDiskState.backfillState.staged ?? false;
          state = "backfilling" as const;
          break;
        case "Backfilled2":
          staged = config.onDiskState.staged ?? false;
          state = "backfilled" as const;
          break;
        default:
          staged = false;
          state = "done" as const;
      }
      return { fields: config.fields, state, staged };
    }
    case "search": {
      const stateType = config.onDiskState.state;
      const state =
        stateType === "backfilling" || stateType === "backfilling2"
          ? ("backfilling" as const)
          : stateType === "backfilled" || stateType === "backfilled2"
            ? ("backfilled" as const)
            : ("done" as const);
      const fields = {
        searchField: config.searchField,
        filterFields: config.filterFields,
      };
      const staged =
        stateType === "backfilling" ||
        stateType === "backfilling2" ||
        stateType === "backfilled2"
          ? (config.onDiskState.staged ?? false)
          : false;
      return {
        fields,
        state,
        staged,
      };
    }
    case "vector": {
      const stateType = config.onDiskState.state;
      const state =
        stateType === "backfilling"
          ? ("backfilling" as const)
          : stateType === "backfilled" || stateType === "backfilled2"
            ? ("backfilled" as const)
            : ("done" as const);
      const staged =
        stateType === "backfilling" ||
        stateType === "backfilled" ||
        stateType === "backfilled2"
          ? (config.onDiskState.staged ?? false)
          : false;
      return {
        fields: {
          vectorField: config.vectorField,
          filterFields: config.filterFields,
          dimensions: Number(config.dimensions),
        },
        state,
        staged,
      };
    }
    default: {
      config satisfies never;
      throw new Error(`Unknown index type`);
    }
  }
}

export default queryPrivateSystem("ViewData")({
  args: {
    tableName: v.optional(v.union(v.string(), v.null())),
    // Pass the `componentId` for this arg.
    // Note that this arg is named `tableNamespace` not `componentId` because if it is `componentId`,
    // the queries will be executed within the component's table namespace,
    // which doesn't have the `_index` or `_index_backfills` tables
    // We only need this argument to get the correct tableId.
    tableNamespace: v.union(v.string(), v.null()),
  },
  handler: async ({ db }, { tableName, tableNamespace }) => {
    if (!tableName) {
      return undefined;
    }
    const tableId = await getTableId(db, tableName, tableNamespace);
    if (!tableId) {
      return undefined;
    }
    const indexes = await db
      .query("_index")
      .withIndex("by_id", (q) => q)
      // eslint-disable-next-line @convex-dev/no-filter-in-query -- FIXME
      .filter((q) => q.eq(q.field("table_id"), tableId))
      .collect();
    const userIndexes = indexes.filter(
      (index) =>
        index.descriptor !== "by_id" && index.descriptor !== "by_creation_time",
    );
    return Promise.all(
      userIndexes.map(async (index) => {
        const { fields, state, staged } = indexFieldsAndState(index.config);
        if (state === "backfilling") {
          const indexBackfill = await db
            .query("_index_backfills")
            .withIndex("by_index_id", (q) => q.eq("indexId", index._id))
            .unique();
          const stats = indexBackfill
            ? {
                numDocsIndexed: Number(indexBackfill.numDocsIndexed),
                totalDocs: indexBackfill.totalDocs
                  ? Number(indexBackfill.totalDocs)
                  : null,
              }
            : undefined;
          return {
            name: index.descriptor,
            staged,
            fields,
            backfill: { state, stats: stats },
          };
        }
        return {
          name: index.descriptor,
          staged,
          fields,
          backfill: { state },
        };
      }),
    );
  },
});

export type BackfillingIndex = {
  tableName: string;
  name: string;
  kind: IndexConfig["type"];
  staged: boolean;
  stats: { numDocsIndexed: number; totalDocs: number | null } | null;
};

/// Every index still backfilling on a root-component table, sorted by table
/// then index name. It walks `_index_backfills`, which only holds in-flight
/// backfills, and fetches each index and table by ID, so it stays cheap on
/// deployments with thousands of tables and indexes. The database index
/// workers and the search flusher both record progress there, so database,
/// search and vector backfills are all listed; `kind` tells them apart.
/// `_index` and `_index_backfills` live only in the root namespace, so this
/// query takes no component argument.
export const backfilling = queryPrivateSystem("ViewData")({
  args: {},
  handler: async ({ db }): Promise<BackfillingIndex[]> => {
    const backfills = await db
      .query("_index_backfills")
      .withIndex("by_id", (q) => q)
      .collect();
    if (backfills.length === 0) {
      return [];
    }
    // `_index.table_id` is a table's raw internal ID, and any `_tables`
    // document ID carries the number that turns it back into a document ID.
    const someTable = await db
      .query("_tables")
      .withIndex("by_id", (q) => q)
      .first();
    if (someTable === null) {
      throw new Error(
        "_tables is empty while _index_backfills is not: every backfilling index belongs to a table",
      );
    }
    const tablesTableNumber = decodeId(someTable._id).tableNumber;
    const result: BackfillingIndex[] = [];
    for (const backfill of backfills) {
      const index = await db.get(backfill.indexId);
      if (
        index === null ||
        index.descriptor === "by_id" ||
        index.descriptor === "by_creation_time"
      ) {
        continue;
      }
      const { state, staged } = indexFieldsAndState(index.config);
      if (state !== "backfilling") {
        continue;
      }
      const table = await tableByInternalId(
        db,
        tablesTableNumber,
        index.table_id,
      );
      if (
        table === null ||
        table.state !== "active" ||
        table.namespace !== undefined
      ) {
        continue;
      }
      result.push({
        tableName: table.name,
        name: index.descriptor,
        kind: index.config.type,
        staged,
        stats: {
          numDocsIndexed: Number(backfill.numDocsIndexed),
          totalDocs:
            backfill.totalDocs === null ? null : Number(backfill.totalDocs),
        },
      });
    }
    return result.sort(
      (a, b) =>
        a.tableName.localeCompare(b.tableName) || a.name.localeCompare(b.name),
    );
  },
});

/// The `_tables` document whose ID wraps `internalId`, the URL-safe base64
/// form `_index.table_id` uses, or null when the ID is not one.
async function tableByInternalId(
  db: GenericDatabaseReader<DataModel>,
  tablesTableNumber: number,
  internalId: string,
): Promise<Doc<"_tables"> | null> {
  const standardBase64 = internalId.replace(/-/g, "+").replace(/_/g, "/");
  const padded =
    standardBase64 + "=".repeat((4 - (standardBase64.length % 4)) % 4);
  let id: string;
  try {
    id = encodeId(tablesTableNumber, Base64.toByteArray(padded));
  } catch {
    return null;
  }
  return db.get(id as Id<"_tables">);
}
