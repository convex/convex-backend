import { cronJobs } from "convex/server";
import { api } from "./_generated/api";

const crons = cronJobs();

crons.interval("15s task", { seconds: 10 }, api.sendEmail.default);

crons.interval(
  "clear presence data",
  { seconds: 10 },
  api.clearPresence.default,
  {
    a: 12,
  },
);

crons.hourly(
  "Clear at the top of the hour",
  {
    // eslint-disable-next-line @convex-dev/no-top-of-hour-crons -- exercises the on-the-hour path
    minuteUTC: 0,
  },
  api.clearMessage.default,
  { n: 1 },
);

crons.daily(
  "Daily high score reset",
  {
    hourUTC: 17, // (9:30am Pacific/10:30am Daylight Savings Pacific)
    minuteUTC: 30, // no timezone support yet
  },
  api.clearHighScore.default,
);

crons.weekly(
  "Weekly re-engagement email",
  {
    dayOfWeek: "tuesday",
    hourUTC: 17, // (9:30am Pacific/10:30am Daylight Savings Pacific)
    minuteUTC: 30, // no timezone support yet
  },
  api.sendEmail.default,
);

crons.monthly(
  "Clear a message once a month",
  {
    day: 1,
    hourUTC: 17, // (9:30am Pacific/10:30am Daylight Savings Pacific)
    minuteUTC: 30, // no timezone support yet
  },
  api.clearMessage.default,
  { n: 1 },
);

// eslint-disable-next-line @convex-dev/no-top-of-hour-crons -- exercises the on-the-hour path
crons.cron("clear a message", "0 10 * * 2", api.clearMessage.default, { n: 1 });
crons.cron(
  "fancier cron job!",
  "10-30/5 10 1-3 * *",
  api.clearMessage.default,
  {
    n: 1,
  },
);

crons.interval("record time", { seconds: 1 }, api.recordTime.default);

export default crons;
