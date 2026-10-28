export type VideoWebhookEvent = {
  id: string;
  type: string;
  status: "completed" | "failed" | "cancelled" | "expired";
  operation: string;
};

/** Pass the unmodified request body: the signature covers its exact bytes. */
export async function verifyVideoWebhook({
  body,
  signature,
  secret,
  now = Date.now(),
}: {
  body: string;
  signature: string | null;
  secret: string;
  now?: number;
}): Promise<VideoWebhookEvent> {
  const match = signature?.match(/^t=(\d+),v1=([0-9a-f]{64})$/);
  const timestamp = Number(match?.[1]);
  if (
    !match ||
    !secret ||
    !Number.isSafeInteger(timestamp) ||
    !Number.isFinite(now) ||
    Math.abs(now / 1000 - timestamp) > 300
  ) {
    throw new Error("Invalid or expired video webhook signature");
  }
  const encoder = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["verify"],
  );
  const mac = Uint8Array.from(match[2].match(/../g)!, (byte) =>
    parseInt(byte, 16),
  );
  const valid = await crypto.subtle.verify(
    "HMAC",
    key,
    mac,
    encoder.encode(`${match[1]},${body}`),
  );
  if (!valid) throw new Error("Invalid video webhook signature");
  const event = JSON.parse(body) as VideoWebhookEvent | null;
  if (
    !event ||
    typeof event.id !== "string" ||
    !event.id ||
    typeof event.operation !== "string" ||
    !event.operation ||
    !["completed", "failed", "cancelled", "expired"].includes(event.status) ||
    event.type !== `video.generation.${event.status}`
  ) {
    throw new Error("Invalid video webhook payload");
  }
  return event;
}
