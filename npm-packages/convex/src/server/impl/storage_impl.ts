import {
  FileMetadata,
  StorageActionWriter,
  FileStorageId,
  StorageReader,
  StorageWriter,
} from "../storage.js";
import { version } from "../../index.js";
import { fromByteArray, toByteArray } from "../../values/base64.js";
import { performAsyncSyscall, performJsSyscall } from "./syscall.js";
import { validateArg } from "./validate.js";

export function setupStorageReader(requestId: string): StorageReader {
  return {
    getUrl: async (storageId: FileStorageId) => {
      validateArg(storageId, 1, "getUrl", "storageId");
      return await performAsyncSyscall("1.0/storageGetUrl", {
        requestId,
        version,
        storageId,
      });
    },
    getMetadata: async (storageId: FileStorageId): Promise<FileMetadata> => {
      return await performAsyncSyscall("1.0/storageGetMetadata", {
        requestId,
        version,
        storageId,
      });
    },
  };
}

export function setupStorageWriter(requestId: string): StorageWriter {
  const reader = setupStorageReader(requestId);
  return {
    generateUploadUrl: async () => {
      return await performAsyncSyscall("1.0/storageGenerateUploadUrl", {
        requestId,
        version,
      });
    },
    delete: async (storageId: FileStorageId) => {
      await performAsyncSyscall("1.0/storageDelete", {
        requestId,
        version,
        storageId,
      });
    },
    store: async (blob: Blob, options?: { sha256?: string }) => {
      if (!(blob instanceof Blob)) {
        throw new Error(
          "store() expects a Blob. If you are trying to store a Request, `await request.blob()` will give you the correct input.",
        );
      }
      const bytes = new Uint8Array(await blob.arrayBuffer());
      return await performAsyncSyscall("1.0/storageStore", {
        requestId,
        version,
        blob: fromByteArray(bytes),
        contentType: blob.type,
        sha256: options?.sha256,
      });
    },
    get: async (storageId: FileStorageId) => {
      const result: { blob: string; contentType: string | null } | null =
        await performAsyncSyscall("1.0/storageGet", {
          requestId,
          version,
          storageId,
        });
      if (result === null) {
        return null;
      }
      return new Blob([toByteArray(result.blob)], {
        type: result.contentType ?? "",
      });
    },
    getUrl: reader.getUrl,
    getMetadata: reader.getMetadata,
  };
}

export function setupStorageActionWriter(
  requestId: string,
): StorageActionWriter {
  const writer = setupStorageWriter(requestId);
  return {
    ...writer,
    store: async (blob: Blob, options?: { sha256?: string }) => {
      return await performJsSyscall("storage/storeBlob", {
        requestId,
        version,
        blob,
        options,
      });
    },
    get: async (storageId: FileStorageId) => {
      return await performJsSyscall("storage/getBlob", {
        requestId,
        version,
        storageId,
      });
    },
  };
}
