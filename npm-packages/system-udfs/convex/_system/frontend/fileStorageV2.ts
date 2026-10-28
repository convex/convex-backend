import { PaginationResult, SystemDataModel } from "convex/server";
import { mutationGeneric, writeAuditLog } from "../server";
import { Id } from "../../_generated/dataModel";
import { v } from "convex/values";
import { paginationOptsValidator } from "convex/server";
import { queryGeneric } from "../secretSystemTables";

export const numFiles = queryGeneric("ViewData")({
  args: { componentId: v.optional(v.union(v.string(), v.null())) },
  handler: async ({ db }): Promise<number> => {
    return await db.system.query("_storage").count();
  },
});

export type FileMetadata = SystemDataModel["_storage"]["document"] & {
  url: string;
};

export const fileMetadata = queryGeneric("ViewData")({
  args: {
    paginationOpts: paginationOptsValidator,
    filters: v.optional(
      v.object({
        minCreationTime: v.optional(v.number()),
        maxCreationTime: v.optional(v.number()),
        order: v.optional(v.union(v.literal("asc"), v.literal("desc"))),
      }),
    ),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async (
    { db, storage },
    { filters, paginationOpts },
  ): Promise<PaginationResult<FileMetadata>> => {
    const query = db.system.query("_storage");
    const hasDateFilters =
      filters &&
      (filters.minCreationTime !== undefined ||
        filters.maxCreationTime !== undefined);
    const queryWithDateFilters = hasDateFilters
      ? query.withIndex("by_creation_time", (q) => {
          let partial: any = q;
          if (filters.minCreationTime !== undefined) {
            partial = q.gte("_creationTime", filters.minCreationTime);
          }

          if (filters.maxCreationTime !== undefined) {
            return partial.lte("_creationTime", filters.maxCreationTime);
          }
          return partial;
        })
      : query;
    const files = await queryWithDateFilters
      .order(filters?.order ?? "desc")
      .paginate(paginationOpts);

    const newPage = await Promise.all(
      files.page.map(async (file) => {
        const url = (await storage.getUrl(file._id))!;
        return {
          url,
          ...file,
        };
      }),
    );
    return {
      ...files,
      page: newPage,
    };
  },
});

export const getFile = queryGeneric("ViewData")({
  args: {
    storageId: v.string(),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async (
    { db, storage },
    { storageId },
  ): Promise<FileMetadata | null> => {
    const file = await db.system.get(storageId as Id<"_storage">);
    if (!file) {
      return null;
    }
    const url = (await storage.getUrl(file._id))!;
    return {
      url,
      ...file,
    };
  },
});

export const deleteFile = mutationGeneric("WriteData")({
  args: {
    storageId: v.id("_storage"),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async (
    { storage },
    { storageId }: { storageId: Id<"_storage"> },
  ): Promise<void> => {
    await storage.delete(storageId);
    await writeAuditLog("delete_files", {
      storage_ids: [storageId],
    });
  },
});

export const deleteFiles = mutationGeneric("WriteData")({
  args: {
    storageIds: v.array(v.id("_storage")),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async ({ storage }, { storageIds }): Promise<void> => {
    for (const storageId of storageIds) {
      await storage.delete(storageId);
    }
    await writeAuditLog("delete_files", {
      storage_ids: storageIds,
    });
  },
});

export const generateUploadUrl = mutationGeneric("WriteData")({
  args: { componentId: v.optional(v.union(v.string(), v.null())) },
  handler: async ({ storage }): Promise<string> => {
    const url = await storage.generateUploadUrl();
    await writeAuditLog("generate_upload_url", {});
    return url;
  },
});
