export const MAX_IN_FLIGHT_PER_SCENARIO = 1024;
export const RATE_REPORT_EVERY = 500;

export type RateStats = {
  issued: number;
  missed: number;
  inFlight: number;
};

type RateRunnerOptions = {
  rate: number;
  run: () => Promise<void>;
  onError: (error: unknown) => void;
  onReport: (stats: RateStats) => void;
  maxInFlight?: number;
  reportEvery?: number;
  now?: () => number;
  sleep?: (durationMs: number) => Promise<void>;
  signal?: AbortSignal;
};

export function validateRate(rate: number) {
  if (!Number.isFinite(rate) || rate < 0) {
    throw new Error(`Rate must be finite and non-negative, got ${rate}`);
  }
}

export async function runAtRate({
  rate,
  run,
  onError,
  onReport,
  maxInFlight = MAX_IN_FLIGHT_PER_SCENARIO,
  reportEvery = RATE_REPORT_EVERY,
  now = Date.now,
  sleep = (durationMs) =>
    new Promise<void>((resolve) => setTimeout(resolve, durationMs)),
  signal,
}: RateRunnerOptions) {
  validateRate(rate);
  if (rate === 0) {
    return;
  }

  const period = 1000 / rate;
  let nextAt = now();
  let inFlight = 0;
  let missed = 0;
  let issued = 0;
  let nextReportAt = reportEvery;

  while (!signal?.aborted) {
    nextAt += period;
    const beforeWait = now();
    if (beforeWait < nextAt) {
      await sleep(nextAt - beforeWait);
      if (signal?.aborted) {
        break;
      }

      const afterWait = now();
      const skippedWhileWaiting = Math.floor((afterWait - nextAt) / period);
      if (skippedWhileWaiting > 0) {
        missed += skippedWhileWaiting;
        nextAt = afterWait;
      }
    } else if (beforeWait > nextAt) {
      missed += Math.floor((beforeWait - nextAt) / period) + 1;
      nextAt = beforeWait;
    }

    if (inFlight >= maxInFlight) {
      missed += 1;
    } else {
      inFlight += 1;
      issued += 1;
      let promise: Promise<void> | undefined;
      try {
        promise = run();
      } catch (error) {
        inFlight -= 1;
        onError(error);
      }
      if (promise) {
        void promise
          .catch((error) => onError(error))
          .finally(() => {
            inFlight -= 1;
          });
      }
    }

    const accounted = issued + missed;
    while (accounted >= nextReportAt) {
      onReport({ issued, missed, inFlight });
      nextReportAt += reportEvery;
    }
  }
}

type RunnableScenario<Client> = {
  run(client: Client): Promise<void>;
  cleanUp(): void;
};

export async function runScenarioOnce<
  Client,
  T extends RunnableScenario<Client>,
>(
  createScenario: () => T,
  client: Client,
  onError: (scenario: T, error: unknown) => void,
) {
  const scenario = createScenario();
  try {
    await scenario.run(client);
  } catch (error) {
    onError(scenario, error);
  } finally {
    scenario.cleanUp();
  }
}
