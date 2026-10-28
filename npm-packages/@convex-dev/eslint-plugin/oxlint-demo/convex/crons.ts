import { cronJobs } from "convex/server";
import { internal } from "./_generated/api";

const crons = cronJobs();

// oxlint-disable-next-line @convex-dev/no-top-of-hour-crons
crons.hourly("digest", { minuteUTC: 0 }, internal.digest.send);

export default crons;
