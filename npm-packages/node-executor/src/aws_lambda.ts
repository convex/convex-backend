import { Context } from "aws-lambda";
import { invoke, ogProcessExit } from "./executor";
import { logDebug, setDebugLogging } from "./log";
import { populatePrebuildPackages } from "./source_package";
import { finished, Writable } from "node:stream";

const warmupPromise = populatePrebuildPackages();

declare const awslambda: any;

const CALLBACK_WAIT_FOR_EMPTY_EVENT_LOOP =
  process.env.CALLBACK_WAITS_FOR_EMPTY_EVENT_LOOP === "true";

const MAX_INVOKE_COUNT = process.env.MAX_INVOKE_COUNT
  ? parseInt(process.env.MAX_INVOKE_COUNT)
  : 16;

export const handler = awslambda.streamifyResponse(
  async (event: any, responseStream: Writable, context: Context) => {
    // Using `streamifyResponse()` changes the behavior of the Lambda VM
    // as compared to promise/async API (and actually makes it more like the
    // original callback Lambda API): instead of always freezing after the
    // promise of this function resolves it remaining active if there are any
    // setTimeout timers, etc. pending.
    //
    // Setting `callbackWaitsForEmptyEventLoop` to false restores that behavior
    // of freezing the Firecracker VM until the next function invocation at
    // which point such scheduled functions may run.
    //
    // The main benefit of this is not being billed for 10 minutes of
    // Lambda execution when the streamed response was returned long
    // before that, often in a couple seconds.
    //
    // The downside is loss of isolation between invocations:
    // timers may fire the next time the runtime is used.
    context.callbackWaitsForEmptyEventLoop = CALLBACK_WAIT_FOR_EMPTY_EVENT_LOOP;

    setDebugLogging(true);
    await warmupPromise;
    event.requestId = context.awsRequestId;
    const numInvocations = await invoke(event, responseStream);
    if (
      (event.type === "analyze" || event.type === "build_deps") &&
      numInvocations >= MAX_INVOKE_COUNT
    ) {
      logDebug(
        `analyze or build_deps ran ${numInvocations} times, restarting node process`,
      );
      // Exiting before the response flushes truncates it. `finished` also
      // settles on error/close, so a destroyed stream still restarts.
      finished(responseStream, () => ogProcessExit(0));
    }
    responseStream.end();
  },
);
