const DEFAULT_PROCESS_TITLE = "convex-node-executor";
// Keep process titles readable in Activity Monitor and below platform truncation limits.
const MAX_PROCESS_TITLE_BYTES = 96;

export function nodeExecutorProcessTitle(override?: string): string {
  const normalized = override?.replace(/[\s\p{C}]+/gu, " ").trim();
  const title = normalized || DEFAULT_PROCESS_TITLE;
  let byteLength = 0;
  let truncated = "";
  for (const character of title) {
    const characterByteLength = Buffer.byteLength(character);
    if (byteLength + characterByteLength > MAX_PROCESS_TITLE_BYTES) {
      break;
    }
    truncated += character;
    byteLength += characterByteLength;
  }
  return truncated;
}

export function setNodeExecutorProcessTitle(
  target: { title: string },
  override?: string,
): void {
  target.title = nodeExecutorProcessTitle(override);
}
