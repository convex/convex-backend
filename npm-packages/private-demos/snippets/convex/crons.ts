import { cronJobs } from "convex/server";
import { internal } from "./_generated/api";

const crons = cronJobs();

crons.interval(
  "clear messages table",
  { minutes: 1 }, // every minute
  internal.messages.clearAll,
);

crons.monthly(
  "payment reminder",
  { day: 1, hourUTC: 16 }, // Convex chooses the minute within the 16:00 UTC hour
  internal.payments.sendPaymentEmail,
  { email: "my_email@gmail.com" }, // argument to sendPaymentEmail
);

export default crons;
