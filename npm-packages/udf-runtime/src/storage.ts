import { constructStreamId, extractStream } from "./06_streams.js";
import { performAsyncOp } from "./syscall.js";
import { Blob } from "./09_file.js";

export const storeBlob = async ({
  blob,
  options,
}: {
  blob: Blob;
  options?: { sha256?: string };
}) => {
  if (!(blob instanceof Blob)) {
    throw new Error(
      "store() expects a Blob. If you are trying to store a Request, `await request.blob()` will give you the correct input.",
    );
  }
  const bodyStream = blob.stream();
  const streamId = bodyStream ? constructStreamId(bodyStream) : null;
  const digestHeader =
    options?.sha256 !== undefined ? `sha-256=${options?.sha256}` : undefined;

  const storageId = await performAsyncOp(
    "storage/store",
    streamId,
    blob.type,
    blob.size.toString(),
    digestHeader,
  );
  return storageId;
};

type ResponseJson = {
  bodyStreamId: string;
  contentType: string | null;
  contentLength: number;
};

export const getBlob = async ({ storageId }: { storageId: string }) => {
  if (typeof storageId !== "string") {
    throw new Error(
      `storage.get requires a string storageId but received ${storageId}`,
    );
  }
  const responseJsonOrNull: ResponseJson | null = await performAsyncOp(
    "storage/get",
    storageId,
  );
  if (responseJsonOrNull === null) {
    return null;
  }
  const contentType = responseJsonOrNull.contentType ?? undefined;
  const size = responseJsonOrNull.contentLength;
  return Blob.fromStream(
    extractStream(responseJsonOrNull.bodyStreamId),
    size,
    contentType,
  );
};
