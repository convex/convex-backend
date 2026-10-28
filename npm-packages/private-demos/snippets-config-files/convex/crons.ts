import { cronJobs } from "convex/server";
import { internal } from "./_generated/api";

const crons = cronJobs();

// Runs every minute
crons.interval(
  "clear messages table",
  { minutes: 1 },
  internal.messages.clearAll,
);

// Runs daily at 17:00 UTC. Convex picks the minute to spread
// the load away from the top of the hour.
crons.daily("send reminder", { hourUTC: 17 }, internal.emails.send);

// Runs on the first day of every month at 16:00 UTC,
// passing an argument to the function
crons.monthly(
  "payment reminder",
  { day: 1, hourUTC: 16 },
  internal.payments.sendPaymentEmail,
  { email: "my_email@gmail.com" },
);

export default crons;
