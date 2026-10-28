import { CronSchedule } from "system-udfs/convex/_system/frontend/common";

export function scheduleAsCron(
  s: CronSchedule & {
    type: "hourly" | "daily" | "weekly" | "monthly" | "cron";
  },
  // When the schedule leaves the minute to Convex (`minuteUTC` omitted), the
  // caller can pass the minute of the actual next run so the description shows
  // the real time rather than defaulting to the top of the hour.
  fallbackMinuteUTC?: number,
): string {
  // TypeScript should catch, check just in case it doesn't.
  if ((s.type as any) === "interval") {
    throw new Error("Can't format an interval schedule as a cron");
  }
  switch (s.type) {
    case "hourly": {
      return `${s.minuteUTC ?? fallbackMinuteUTC ?? 0} * * * *`;
    }
    case "daily": {
      return `${s.minuteUTC ?? fallbackMinuteUTC ?? 0} ${s.hourUTC} * * *`;
    }
    case "weekly": {
      return `${s.minuteUTC ?? fallbackMinuteUTC ?? 0} ${s.hourUTC} * * ${s.dayOfWeek}`;
    }
    case "monthly": {
      return `${s.minuteUTC ?? fallbackMinuteUTC ?? 0} ${s.hourUTC} ${s.day} * *`;
    }
    case "cron": {
      return s.cronExpr;
    }
    default: {
      s satisfies never;
      throw new Error(`Value type not supported: ${s}`);
    }
  }
}

/**
 * Add a reminder that this is UTC
 */
export function prettierSaffron(s: string) {
  return s.replaceAll("AM", "AM UTC").replaceAll("PM", "PM UTC");
}

export function scheduleLiteral(s: CronSchedule): string {
  // An omitted `minuteUTC` means the developer left the minute to Convex, so
  // drop it from the reconstructed call rather than printing `undefined`.
  const minuteLine =
    "minuteUTC" in s && s.minuteUTC !== undefined
      ? `\n  minuteUTC: ${s.minuteUTC},`
      : "";
  switch (s.type) {
    case "interval":
      return `interval({ seconds: ${s.seconds} })`;
    case "hourly":
      return s.minuteUTC !== undefined
        ? `hourly({ minuteUTC: ${s.minuteUTC} })`
        : `hourly()`;
    case "daily":
      return `daily({\n  hourUTC: ${s.hourUTC},${minuteLine}\n})`;
    case "weekly":
      return `weekly({\n  dayOfWeek: ${s.dayOfWeek},\n  hourUTC: ${s.hourUTC},${minuteLine}\n})`;
    case "monthly":
      return `monthly({\n  day: ${s.day},\n  hourUTC: ${s.hourUTC},${minuteLine}\n})`;
    case "cron":
      return `${s.cronExpr}`;
    default:
      return `Unknown Cron Schedule`;
  }
}
