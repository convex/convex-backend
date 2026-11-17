import { GenericId } from "../values/index.js";

/**
 * @deprecated This ID format is no longer returned by stable file storage APIs. Use `Id<"_storage">` instead.
 *
 * The old ID format used by Convex file storage.
 *
 * ⚠️ Security warning: Anyone that has knows to this ID can download the underlying file
 * from `https://<deployment>.convex.cloud/api/storage/<storageId>`.
 * (Note that it’s safe to share the new ID format, `Id<"_storage">`, to anyone).
 *
 * @public
 */
export type StorageId = string;

/**
 * @deprecated This type is only necessary for backwards compatibility with Convex versions that
 * pre-date `convex@1.6.0`. Use `Id<"_storage">` instead.
 */
export type FileStorageId = GenericId<"_storage"> | StorageId;

/**
 * @deprecated This type is only returned by {@link StorageReader.getUrl | storage.getUrl}.
 *   To get the details of a document, use `ctx.db.system.get("_storage", storageId)` instead.
 *
 * Metadata for a single file as returned by {@link StorageReader.getMetadata | storage.getMetadata}.
 *
 * @public
 */
export type FileMetadata = {
  /**
   * ID for referencing the file (eg. via {@link StorageReader.getUrl | storage.getUrl})
   *
   * This is an older ID format that is no longer returned by stable file storage APIs.
   * Consider using `Id<"_storage">` instead.
   *
   * ⚠️ Security warning: Anyone that has knows to this ID can download the underlying file
   * from `https://<deployment>.convex.cloud/api/storage/<storageId>`.
   * (Note that it’s safe to share the new ID format, `Id<"_storage">`, to anyone).
   */
  storageId: StorageId;
  /**
   * Hex encoded sha256 checksum of file contents
   */
  sha256: string;
  /**
   * Size of the file in bytes
   */
  size: number;
  /**
   * ContentType of the file if it was provided on upload
   */
  contentType: string | null;
};

/**
 * An interface to read files from storage within Convex query functions.
 *
 * Available as `ctx.storage` in queries (read-only), mutations, and actions.
 *
 * @example
 * ```typescript
 * // Get a URL for a stored file:
 * const url = await ctx.storage.getUrl(storageId);
 * if (url) {
 *   // Use the URL (e.g., return it to the client)
 * }
 *
 * // Get file metadata via the system table (preferred over deprecated getMetadata):
 * const metadata = await ctx.db.system.get("_storage", storageId);
 * // metadata: { _id, _creationTime, sha256, size, contentType? }
 * ```
 *
 * @see https://docs.convex.dev/file-storage
 * @public
 */
export interface StorageReader {
  /**
   * Get the URL for a file in storage by its `Id<"_storage">`.
   *
   * The GET response includes a standard HTTP Digest header with a sha256 checksum.
   *
   * @example
   * ```typescript
   * const url = await ctx.storage.getUrl(storageId);
   * ```
   *
   * @param storageId - The `Id<"_storage">` of the file to fetch from Convex storage.
   * @returns - A URL which fetches the file via an HTTP GET, or `null` if the file no longer exists.
   */
  getUrl(storageId: GenericId<"_storage">): Promise<string | null>;

  /**
   * @deprecated Passing a string is deprecated, use `storage.getUrl(Id<"_storage">)` instead.
   *
   * Get the URL for a file in storage by its {@link StorageId}.
   *
   * The GET response includes a standard HTTP Digest header with a sha256 checksum.
   *
   * @param storageId - The {@link StorageId} of the file to fetch from Convex storage.
   * @returns - A url which fetches the file via an HTTP GET, or `null` if it no longer exists.
   */
  getUrl<T extends StorageId>(
    storageId: T extends { __tableName: any } ? never : T,
  ): Promise<string | null>;

  /**
   * @deprecated Use `ctx.db.system.get("_storage", storageId)` instead, which returns
   * equivalent metadata from the `_storage` system table (with a slightly different shape):
   * ```typescript
   * const metadata = await ctx.db.system.get("_storage", storageId);
   * // { _id, _creationTime, sha256, size, contentType? }
   * ```
   *
   * Get metadata for a file.
   *
   * @param storageId - The `Id<"_storage">` of the file.
   * @returns - A {@link FileMetadata} object if found or `null` if not found.
   */
  getMetadata(storageId: GenericId<"_storage">): Promise<FileMetadata | null>;

  /**
   * @deprecated Use `ctx.db.system.get("_storage", storageId)` instead.
   *
   * Get metadata for a file.
   *
   * @param storageId - The {@link StorageId} of the file.
   * @returns - A {@link FileMetadata} object if found or `null` if not found.
   */
  getMetadata<T extends StorageId>(
    storageId: T extends { __tableName: any } ? never : T,
  ): Promise<FileMetadata | null>;
}

/**
 * An interface to read and write files to storage within Convex mutation functions.
 *
 * Available as `ctx.storage` in mutations, actions, and HTTP actions.
 *
 * @see https://docs.convex.dev/file-storage
 * @public
 */
export interface StorageWriter extends StorageReader {
  /**
   * Generate a short-lived URL for uploading a file into storage.
   *
   * The client should make a POST request to this URL with the file as the
   * body. The response will be a JSON object containing a newly allocated
   * `Id<"_storage">` (`{ storageId: "..." }`).
   *
   * @example
   * ```typescript
   * // In a mutation, generate the upload URL:
   * export const generateUploadUrl = mutation({
   *   args: {},
   *   returns: v.string(),
   *   handler: async (ctx) => {
   *     return await ctx.storage.generateUploadUrl();
   *   },
   * });
   *
   * // On the client, upload the file:
   * // const uploadUrl = await generateUploadUrl();
   * // const result = await fetch(uploadUrl, { method: "POST", body: file });
   * // const { storageId } = await result.json();
   * ```
   *
   * @returns - A short-lived URL for uploading a file via HTTP POST.
   */
  generateUploadUrl(): Promise<string>;
  /**
   * Delete a file from Convex storage.
   *
   * Once a file is deleted, any URLs previously generated by {@link StorageReader.getUrl} will return 404s.
   *
   * @example
   * ```typescript
   * await ctx.storage.delete(storageId);
   * ```
   *
   * @param storageId - The `Id<"_storage">` of the file to delete from Convex storage.
   */
  delete(storageId: GenericId<"_storage">): Promise<void>;

  /**
   * @deprecated Passing a string is deprecated, use `storage.delete(Id<"_storage">)` instead.
   *
   * Delete a file from Convex storage.
   *
   * Once a file is deleted, any URLs previously generated by {@link StorageReader.getUrl} will return 404s.
   *
   * @param storageId - The {@link StorageId} of the file to delete from Convex storage.
   */
  delete<T extends StorageId>(
    storageId: T extends { __tableName: any } ? never : T,
  ): Promise<void>;

  /**
   * Download a file from storage as a Blob.
   *
   * In mutations, files read count toward the transaction's `filesRead` and
   * `fileReadBytes` limits. Use
   * `ctx.meta.getTransactionMetrics()` to check the remaining headroom.
   *
   * @param storageId - The `Id<"_storage">` of the file.
   * @returns A Blob containing the file contents, or `null` if the file doesn't exist.
   */
  get(storageId: GenericId<"_storage">): Promise<Blob | null>;

  /**
   * @deprecated Passing a string is deprecated, use `storage.get(Id<"_storage">)` instead.
   *
   * Get a Blob containing the file associated with the provided {@link StorageId}, or `null` if there is no file.
   */
  get<T extends StorageId>(
    storageId: T extends { __tableName: any } ? never : T,
  ): Promise<Blob | null>;

  /**
   * Upload a Blob directly to storage.
   *
   * To upload files from a client, use {@link StorageWriter.generateUploadUrl}
   * instead.
   *
   * In mutations, files stored count toward the transaction's `filesWritten`
   * and `fileWriteBytes` limits. Use
   * `ctx.meta.getTransactionMetrics()` to check the remaining headroom.
   *
   * @example
   * ```typescript
   * const storageId = await ctx.storage.store(blob);
   * // If in an action, save storageId to the database via ctx.runMutation
   * ```
   *
   * @param blob - The Blob to store.
   * @param options - Optional settings. Pass `sha256` to verify the file integrity.
   * @returns The `Id<"_storage">` of the newly stored file.
   */
  store(
    blob: Blob,
    options?: { sha256?: string },
  ): Promise<GenericId<"_storage">>;
}

/**
 * An interface to read and write files to storage within Convex actions and HTTP actions.
 *
 * @example
 * ```typescript
 * // In an action, download and re-upload a file:
 * const blob = await ctx.storage.get(storageId);
 * if (blob) {
 *   const newStorageId = await ctx.storage.store(blob);
 * }
 * ```
 *
 * @public
 */
export interface StorageActionWriter extends StorageWriter {}
