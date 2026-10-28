// A module tree that reaches for the host while it evaluates, exporting what it
// saw so a request can prove the snapshot baked the answers in.
declare const process: { env: Record<string, string | undefined> };

const greeting = process.env.GREETING;
const missing = process.env.MISSING;
const again = process.env.GREETING;
const roll = Math.random();
const now = Date.now();
const date = new Date().getTime();

export const baked = {
  isQuery: true,
  invokeQuery: () =>
    JSON.stringify({
      greeting,
      again,
      missingIsUndefined: missing === undefined,
      roll,
      now,
      date,
    }),
};
