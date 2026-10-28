import { cronJobs } from "convex/server";
import { internal } from "./_generated/api";

const crons = cronJobs();

crons.interval(
  "update NPM rules",
  {
    hours: 1,
  },
  internal.npm.refresh,
);

// Convex-evals can iterate quickly which causes guidelines to become invalidated frequently
// this would cause users to be informed about updates too freqently, so this is set to 12
// for a happy medium
crons.interval(
  "update Cursor rules",
  {
    hours: 12,
  },
  internal.cursorRules.refresh,
);

// Convex-evals can iterate quickly which causes guidelines to become invalidated frequently
// this would cause users to be informed about updates too freqently, so this is set to 12
// for a happy medium
crons.interval(
  "update guidelines",
  {
    hours: 12,
  },
  internal.guidelines.refresh,
);

crons.interval(
  "update local backend version",
  {
    hours: 1,
  },
  internal.localBackend.refresh,
);

export default crons;
