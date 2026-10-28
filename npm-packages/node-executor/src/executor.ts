import { Console } from "node:console";
import { Writable } from "node:stream";
import { performance } from "node:perf_hooks";
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { inspect } from "node:util";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

import { DeploymentMetadata, UserIdentity } from "convex/server";

import {
  CanonicalizedModulePath,
  FunctionName,
  isConvexAction,
  UdfPath,
  EnvironmentVariable,
} from "./convex";
import {
  FrameData,
  extractErrorMessage,
  registerPrepareStackTrace,
} from "./errors";
import { findLineNumbers } from "./analyze";
import { Syscalls, SyscallsImpl } from "./syscalls";
import {
  PackageRefcounts,
  SourcePackage,
  maybeDownloadAndLinkPackages,
} from "./source_package";
import { buildDeps, BuildDepsRequest } from "./build_deps";
import { ConvexError, JSONValue } from "convex/values";
import { log, logDebug, logDurationMs } from "./log";
import { AsyncLocalStorage } from "node:async_hooks";
import { countEgressBytes } from "./bytesCounter";

// Small hack to detect if we're running in the dynamic or static lambda.
const AWS_LAMBDA_EXECUTOR_TYPE = (
  process.env.AWS_LAMBDA_FUNCTION_NAME ?? ""
).endsWith("-d")
  ? "DYNAMIC"
  : "STATIC";

// AWS_LAMBDA_FUNCTION_NAME is a reserved environment variable that the Lambda
// runtime always sets and user configuration cannot override:
// https://docs.aws.amazon.com/lambda/latest/dg/configuration-envvars.html
// It must be read at module load time, before invocations replace `process.env`.
const IS_AWS_LAMBDA = process.env.AWS_LAMBDA_FUNCTION_NAME !== undefined;

const AWS_LAMBDA_FUNCTION_MEMORY_SIZE = parseInt(
  process.env.AWS_LAMBDA_FUNCTION_MEMORY_SIZE ?? "512",
  10,
);

// We allocate extra memory (4GB) for dynamic lambdas, but don't want to charge customers for this implementation detail.
// We report 512MB (which may be an undercharge) but this should only be a temporary state while the static lambda deploys.
const AWS_LAMBDA_BILLED_MEMORY_SIZE =
  AWS_LAMBDA_EXECUTOR_TYPE === "DYNAMIC"
    ? 512
    : AWS_LAMBDA_FUNCTION_MEMORY_SIZE;

// When we bundle commonJS modules as ESM with esbuild, the bundled code might still use
// `require`, exports, module, __dirname or __filename despite being in ESM.
//
// We inject these into the environment to make this code work. When actual CJS modules
// are invoked, Node overrides these globals with the correct globals for that module, so
// we don't need to worry about these globals messing with CJS runtime globals.
// See https://nodejs.org/api/modules.html#the-module-wrapper for details on how Node does this.
export function setupGlobals(modulePath: string) {
  // Set `require` to use a module resolution algorithm relative to the module,
  // instead of this executor package, so external deps can be used.
  globalThis.require = createRequire(modulePath);
  globalThis.exports = exports;
  globalThis.module = module;
  // TODO(presley): Currently, __filename and __dirname are /var/task/aws_lambda.cjs
  // and /var/task respectively. Once we use `npm install` to install node_modules
  // instead of bundling them, we can explore dropping those or making them accurate.
  globalThis.__dirname = __dirname;
  globalThis.__filename = __filename;
}

let numInvocations = 0;

async function runWithEnvironmentVariables<T>(
  envs: EnvironmentVariable[],
  fn: (envHash: string) => Promise<T>,
): Promise<T> {
  const savedEnv = process.env;

  // AWS Lambda populates a number of environment variables, like Lambda version,
  // handler name, session, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, etc. We
  // don't want to expose any of that. Only expose variables that are common
  // between local Node.js and AWS Lambda.
  //
  // Note: This sanitization is for consistency between environments, not for security.
  // While user code can still access underlying environment variables through
  // other means, sensitive variables (such as AWS credentials) are protected
  // through restrictive IAM policies that limit what the Lambda function can do
  // with those credentials.
  const allowedEnvs = ["PATH", "PWD", "LANG", "NODE_PATH", "TZ", "UTC"];
  const sanitized: { [name: string]: string } = {};
  for (const name of allowedEnvs) {
    const value = process.env[name];
    if (value !== undefined) {
      sanitized[name] = value;
    }
  }
  process.env = sanitized;

  // Set the user defined environment variables
  envs.sort((a, b) => a.name.localeCompare(b.name));
  for (const e of envs) {
    process.env[e.name] = e.value;
  }

  // Compute a hash based on the user defined environment variables.
  const envHash = createHash("md5").update(JSON.stringify(envs)).digest("hex");

  try {
    return await fn(envHash);
  } finally {
    // Restore the initial environment when we’re done.
    // This is helpful to bypass a AWS Lambda bug affecting Node.js 24 where the lambda
    // would crash when AWS’s own env vars are missing after a second function execution.
    //
    // We don’t restore when running in the local Node executor because this one accepts
    // concurrent requests in the same JavaScript environment.
    if (IS_AWS_LAMBDA) {
      process.env = savedEnv;
    }
  }
}

export const ogProcessExit = process.exit;

function unhandledRejectionHandler(
  responseStream: Writable,
  event: "unhandledRejection" | "uncaughtException" | "process.exit",
  e: unknown,
) {
  // Respond with a user error.
  log(`handling ${event}`);
  const egressBytes = countEgressBytes();
  const response = {
    type: "error",
    message: extractErrorMessage(e),
    name: event,
    frames: [],
    syscallTrace:
      (globalSyscalls.getStore() as SyscallsImpl | null)?.syscallTrace ?? {},
    memoryAllocatedMb: AWS_LAMBDA_FUNCTION_MEMORY_SIZE,
    egressBytes,
    numInvocations,
    totalExecutorTimeMs: 0,
    exitingProcess: true,
  };
  if (e instanceof Error) {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    e.stack; // calls overridden prepareStackTrace
    if ((e as any).__frameData) {
      response.frames = JSON.parse((e as any).__frameData);
    }
  }
  const json = JSON.stringify(response);
  log(json);
  // Use `.end()` to make sure that no other finish message makes it into the
  // stream, then exit the process to prevent any ongoing async work from
  // leaking into the next invocation.
  responseStream.on("finish", () => ogProcessExit(1));
  responseStream.end(json);
}

export async function invoke(
  request: ExecuteRequest | AnalyzeRequest | BuildDepsRequest,
  responseStream: Writable,
): Promise<number> {
  process.removeAllListeners("unhandledRejection");
  process.removeAllListeners("uncaughtException");
  process.on("unhandledRejection", (e: unknown) =>
    unhandledRejectionHandler(responseStream, "unhandledRejection", e),
  );
  process.on("uncaughtException", (e: unknown) =>
    unhandledRejectionHandler(responseStream, "uncaughtException", e),
  );
  process.exit = function processExit(code?: number): never {
    unhandledRejectionHandler(
      responseStream,
      "process.exit",
      new Error(
        `process.exit() called ${code === undefined ? "with no exit code" : `with exit code ${code}`}`,
      ),
    );
    throw new Error("unreachable (unhandledRejectionHandler never returns)");
  };

  const start = performance.now();
  numInvocations += 1;
  logDebug(`Environment numInvocations=${numInvocations}`);
  const result = await globalConsoleState.run(
    defaultConsoleState(
      request.type === "execute" ? request.maxLogLines : undefined,
    ),
    async () => {
      const devConsole = setupConsole(responseStream);
      return await globalDevConsole.run(devConsole, async () => {
        let result;
        if (request.type === "execute") {
          result = await execute(request);
        } else if (request.type === "analyze") {
          result = await analyze(request);
        } else if (request.type === "build_deps") {
          result = await buildDeps(request);
        } else {
          throw new Error(`Unknown request type ${request}`);
        }
        return result;
      });
    },
  );

  logDurationMs("Total invocation time", start);
  logDebug(`Memory allocated: ${AWS_LAMBDA_FUNCTION_MEMORY_SIZE}MB`);
  if (
    AWS_LAMBDA_EXECUTOR_TYPE === "DYNAMIC" &&
    AWS_LAMBDA_BILLED_MEMORY_SIZE !== AWS_LAMBDA_FUNCTION_MEMORY_SIZE
  ) {
    logDebug(
      `Dynamic executor used: reporting ${AWS_LAMBDA_BILLED_MEMORY_SIZE}MB of billed memory`,
    );
  }
  responseStream.write(JSON.stringify(result));
  return numInvocations;
}

export type ExecuteRequest = {
  type: "execute";
  // The AWS lambda request id unique to this particular UDF. Unlike the ID in the ExecutionContext,
  // it's unique to this particular request and never re-used.
  // TODO(CX-5733): Rename this in callers and migrate.
  requestId: string;
  sourcePackage: SourcePackage;

  udfPath: UdfPath;
  args: string;

  backendAddress: string;
  backendCallbackToken: string;
  authHeader: string | null;
  userIdentity: UserIdentity | null;
  environmentVariables: EnvironmentVariable[];
  timeoutSecs: number;
  maxLogLines?: number;
  serviceTokenGuaranteedLifetimeSecs: number;
  npmVersion: string | null;
  executionContext: ExecutionContext;
  encodedParentTrace: string | null;
  deployment: DeploymentMetadata;
};

export type ExecutionContext = {
  requestId: string;
  executionId: string | undefined;
  isRoot: boolean | undefined;
  parentScheduledJob: string | null;
  parentScheduledJobComponentId: string | null;
  ip: string | null;
  userAgent: string | null;
};

export type ExecuteResponseInner =
  | {
      type: "success";
      udfReturn: string;
      udfTimeMs: number;
      importTimeMs: number;
    }
  | {
      type: "error";
      message: string;
      name: string;
      data?: string;
      frames?: FrameData[];
      udfTimeMs?: number;
      importTimeMs?: number;
      exitingProcess: boolean;
    };

export type SyscallStats = {
  invocations: number;
  errors: number;
  totalDurationMs: number;
};

export type ExecuteResponse = ExecuteResponseInner & {
  // The number of invocations in the lifetime of executor environment. 1 implies
  // this is the first request in that environment.
  numInvocations: number;
  // Time spent downloading the package in seconds.
  downloadTimeMs?: number;
  // Time spent compiling the package in seconds.
  importTimeMs?: number;
  // Total time spent in the executor
  totalExecutorTimeMs: number;

  syscallTrace: Record<string, SyscallStats>;

  // The amount of memory allocated to the executor environment. This is constant for the lifetime of the environment.
  memoryAllocatedMb: number;
  // The number of bytes of egress during this request.
  egressBytes: number;
};

export async function execute(
  request: ExecuteRequest,
): Promise<ExecuteResponse> {
  const start = performance.now();
  const context = new PackageRefcounts();
  try {
    // Download missing packages and do any necessary linking
    const local = await maybeDownloadAndLinkPackages(
      context,
      request.sourcePackage,
    );
    const downloadTimeMs = logDurationMs("downloadTime", start);

    const syscalls = new SyscallsImpl(
      request.udfPath,
      request.requestId,
      request.backendAddress,
      request.backendCallbackToken,
      request.authHeader,
      request.userIdentity,
      request.executionContext,
      request.encodedParentTrace,
      request.deployment,
      request.serviceTokenGuaranteedLifetimeSecs * 1000,
    );

    countEgressBytes(); // reset egressBytes counter

    let innerResult: ExecuteResponseInner;
    try {
      if (!local.modules.has(request.udfPath.canonicalizedPath)) {
        throw new Error(
          `Couldn't find module source for ${request.udfPath.canonicalizedPath}`,
        );
      }
      innerResult = await executeInner(
        request.requestId,
        local.dir,
        request.udfPath.canonicalizedPath,
        request.udfPath.function ?? "default",
        request.args,
        request.environmentVariables,
        request.timeoutSecs,
        syscalls,
      );
    } catch (e: any) {
      innerResult = {
        type: "error",
        message: extractErrorMessage(e),
        name: e.name,
        exitingProcess: false,
      };
    } finally {
      // The action has settled. Abort any callbacks still in flight so a dangling
      // (un-awaited) promise can't outlive this invocation and reject into the
      // next, unrelated one that reuses this warm process.
      syscalls.dispose();
    }

    const totalExecutorTimeMs = logDurationMs("totalExecutorTime", start);
    const egressBytes = countEgressBytes();

    return {
      ...innerResult,
      numInvocations,
      downloadTimeMs,
      totalExecutorTimeMs,
      syscallTrace: syscalls.syscallTrace,
      memoryAllocatedMb: AWS_LAMBDA_BILLED_MEMORY_SIZE,
      egressBytes,
    };
  } finally {
    context.dispose();
  }
}

export async function executeInner(
  lambdaExecuteId: string,
  dir: string,
  relPath: string,
  name: FunctionName,
  args: string,
  environmentVariables: EnvironmentVariable[],
  timeoutSecs: number,
  syscalls: Syscalls,
): Promise<ExecuteResponseInner> {
  logDebug(`Executing ${relPath}:${name} from ${dir}`);
  const modulesDir = path.join(dir, "modules");
  registerPrepareStackTrace(modulesDir);
  const start = performance.now();
  // We have to reevaluate the module if the envs change since they can be used
  // in global scope. We add them as query argument to achieve this behavior.
  return await runWithEnvironmentVariables(
    environmentVariables,
    async (envHash) => {
      const modulePath = path.join(modulesDir, relPath);
      setupGlobals(modulePath);
      const moduleUrl = pathToFileURL(modulePath);
      moduleUrl.searchParams.set("envHash", envHash);
      const module = await import(moduleUrl.href);
      const importTimeMs = logDurationMs("importTimeMs", start);

      const userFunction = module[name];
      if (!userFunction) {
        throw new Error(`Couldn't find action \`${name}\` in \`${relPath}\``);
      }
      if (!isConvexAction(userFunction)) {
        throw new Error(
          `\`${name}\` wasn't registered as a Convex action in \`${relPath}\``,
        );
      }
      const invoke = userFunction.invokeAction;
      const startExecute = performance.now();

      // Use this symbol to determine if the result of the Promise.race
      // was a timeout or not.
      const timeoutError = Symbol();
      let udfReturn: string | symbol;
      try {
        let timer: NodeJS.Timeout | null = null;

        const timeout = new Promise<symbol>((res) => {
          timer = setTimeout(() => res(timeoutError), timeoutSecs * 1000);
        });

        udfReturn = await globalSyscalls.run(syscalls, () => {
          return Promise.race<string | symbol>([
            invoke(lambdaExecuteId, args),
            timeout,
          ]).finally(() => {
            // Always clear the timeout after the promise is settled.
            // There shouldn't be a race because the timeout promise is created first.
            // But it's also fine because with Promise.race the timeout promise should be swallowed
            if (timer) {
              clearTimeout(timer);
            }
          });
        });
      } catch (e: any) {
        // Accessing `e.stack` is important! Without it e.__frameData
        // is not generated!
        // eslint-disable-next-line @typescript-eslint/no-unused-expressions
        e?.stack;

        const udfTimeMs = logDurationMs("executeUdf", startExecute);
        return {
          type: "error",
          message: extractErrorMessage(e),
          name: e?.name ?? "",
          data: getConvexErrorData(e),
          frames: e?.__frameData ? JSON.parse(e.__frameData) : [],
          udfTimeMs,
          importTimeMs,
          exitingProcess: false,
        };
      }

      if (udfReturn === timeoutError) {
        throw new Error(
          `Action \`${name}\` execution timed out (maximum duration ${timeoutSecs}s)`,
        );
      }
      if (typeof udfReturn !== "string") {
        throw new Error(
          // Need to cast to a string here to make TS happy.
          `Action \`${name}\` did not return a string (returned \`${String(
            udfReturn,
          )}\`)`,
        );
      }
      syscalls.assertNoPendingSyscalls();
      const udfTimeMs = logDurationMs("executeUdf", startExecute);
      return {
        type: "success",
        udfReturn,
        udfTimeMs,
        importTimeMs,
      };
    },
  );
}

// Keep in sync with registration_impl
function getConvexErrorData(thrown: unknown) {
  if (
    typeof thrown === "object" &&
    thrown !== null &&
    Symbol.for("ConvexError") in thrown
  ) {
    // At this point data has already been serialized
    // in `invokeAction`.
    return (thrown as ConvexError<string>).data;
  }
  return undefined;
}

export type AnalyzeRequest = {
  type: "analyze";
  // The AWS lambda request id unique to this particular request.
  // TODO(CX-5733): Rename this in callers and migrate.
  requestId: string;

  sourcePackage: SourcePackage;
  environmentVariables: EnvironmentVariable[];
};

export type AnalyzeResponse =
  | {
      type: "success";
      modules: Record<CanonicalizedModulePath, AnalyzedFunctions>;
    }
  | {
      type: "error";
      message: string;
      frames?: FrameData[];
    };

export async function analyze(
  request: AnalyzeRequest,
): Promise<AnalyzeResponse> {
  return await runWithEnvironmentVariables(
    request.environmentVariables,
    async () => {
      const context = new PackageRefcounts();
      try {
        const local = await maybeDownloadAndLinkPackages(
          context,
          request.sourcePackage,
        );
        const modulesDir = path.join(local.dir, "modules");
        registerPrepareStackTrace(modulesDir);
        const modules: Record<CanonicalizedModulePath, AnalyzedFunctions> = {};
        for (const modulePath of local.modules) {
          try {
            const filePath = path.join(modulesDir, modulePath);
            modules[modulePath] = await analyzeModule(filePath);
          } catch (e: any) {
            // eslint-disable-next-line @typescript-eslint/no-unused-expressions
            e.stack;
            return {
              type: "error",
              message: `Failed to analyze ${modulePath}: ${extractErrorMessage(e)}`,
              frames: e.__frameData ? JSON.parse(e.__frameData) : [],
            };
          }
        }

        return { type: "success", modules };
      } finally {
        context.dispose();
      }
    },
  );
}

type Visibility = { kind: "public" } | { kind: "internal" };

type UdfType = "action" | "mutation" | "query" | "httpAction";

export type AnalyzedFunctions = Array<{
  name: string;
  lineno: number;
  udfType: UdfType;
  visibility: Visibility | null;
  args: JSONValue | null;
  returns: JSONValue | null;
}>;

async function analyzeModule(filePath: string): Promise<AnalyzedFunctions> {
  setupGlobals(filePath);
  const fileUrl = pathToFileURL(filePath).href;
  const module = await import(fileUrl);

  const functions: Map<
    string,
    {
      udfType: UdfType;
      visibility: Visibility | null;
      args: JSONValue | null;
      returns: JSONValue | null;
    }
  > = new Map();
  for (const [name, value] of Object.entries(module)) {
    if (value === undefined || value === null) {
      continue;
    }

    // TODO: This is a little more permissive than our V8 layer in that we
    // don't check whether `value instanceof Function`. This is tricky here
    // since we need to use the context's `Function` for the prototype check.
    let udfType: UdfType;
    if (
      Object.prototype.hasOwnProperty.call(value, "isAction") &&
      Object.prototype.hasOwnProperty.call(value, "invokeAction")
    ) {
      udfType = "action";
    } else if (
      Object.prototype.hasOwnProperty.call(value, "isQuery") &&
      Object.prototype.hasOwnProperty.call(value, "invokeQuery")
    ) {
      udfType = "query";
    } else if (
      Object.prototype.hasOwnProperty.call(value, "isMutation") &&
      Object.prototype.hasOwnProperty.call(value, "invokeMutation")
    ) {
      udfType = "mutation";
    } else if (
      Object.prototype.hasOwnProperty.call(value, "isHttp") &&
      (Object.prototype.hasOwnProperty.call(value, "invokeHttpEndpoint") ||
        Object.prototype.hasOwnProperty.call(value, "invokeHttpAction"))
    ) {
      udfType = "httpAction";
    } else {
      continue;
    }
    const isPublic = Object.prototype.hasOwnProperty.call(value, "isPublic");
    const isInternal = Object.prototype.hasOwnProperty.call(
      value,
      "isInternal",
    );

    let args: string | null = null;
    if (
      Object.prototype.hasOwnProperty.call(value, "exportArgs") &&
      typeof (value as any).exportArgs === "function"
    ) {
      const exportedArgs = (value as any).exportArgs();
      if (typeof exportedArgs === "string") {
        args = JSON.parse(exportedArgs);
      }
    }
    let returns: string | null = null;
    if (
      Object.prototype.hasOwnProperty.call(value, "exportReturns") &&
      typeof (value as any).exportReturns === "function"
    ) {
      const exportedReturns = (value as any).exportReturns();
      if (typeof exportedReturns === "string") {
        returns = JSON.parse(exportedReturns);
      }
    }

    if (isPublic && isInternal) {
      logDebug(`Skipping function marked as both public and internal: ${name}`);
      continue;
    } else if (isPublic) {
      functions.set(name, {
        udfType,
        visibility: { kind: "public" },
        args,
        returns,
      });
    } else if (isInternal) {
      functions.set(name, {
        udfType,
        visibility: { kind: "internal" },
        args,
        returns,
      });
    } else {
      functions.set(name, { udfType, visibility: null, args, returns });
    }
  }
  // Do an awful, regex based line match that assumes that moduleConfig.source originates from
  // esbuild since we don't have V8's `Function::get_script_line_number` in Node. This was
  // how we did this in `isolate/` before #991.
  const source = fs.readFileSync(filePath, {
    encoding: "utf-8",
  });
  const lineNumbers = findLineNumbers(source, Array.from(functions.keys()));
  const analyzed = [...functions.entries()].map(([name, properties]) => {
    // Finding line numbers is best effort. We should return the analyzed
    // function even if we fail to find the exact line number.
    const lineno = lineNumbers.get(name) ?? 0;
    return {
      name,
      lineno,
      ...properties,
    };
  });

  return analyzed;
}

const globalSyscalls = new AsyncLocalStorage<Syscalls>();
export const globalConsoleState = new AsyncLocalStorage<ConsoleState>();
export const globalDevConsole = new AsyncLocalStorage<Console>();

(globalThis as any).Convex = {
  syscall: (op: string, jsonArgs: string) => {
    const syscalls = globalSyscalls.getStore();
    if (!syscalls) {
      throw new Error(`Cannot invoke syscall during module imports`);
    }
    return syscalls.syscall(op, jsonArgs);
  },
  asyncSyscall: (op: string, jsonArgs: string) => {
    const syscalls = globalSyscalls.getStore();
    if (!syscalls) {
      throw new Error(`Cannot invoke syscall during module imports`);
    }
    return syscalls.asyncSyscall(op, jsonArgs);
  },
  jsSyscall: (op: string, args: Record<string, any>) => {
    const syscalls = globalSyscalls.getStore();
    if (!syscalls) {
      throw new Error(`Cannot invoke syscall during module imports`);
    }
    return syscalls.asyncJsSyscall(op, args);
  },
};

export const ogConsole = globalThis.console;
Object.defineProperty(globalThis, "console", {
  get() {
    return globalDevConsole.getStore()!;
  },
});

function toString(value: unknown, defaultValue: string) {
  return value === undefined
    ? defaultValue
    : value === null
      ? "null"
      : value.toString();
}

const DEFAULT_MAX_LOG_LINES = 1024;

type ConsoleState = {
  maxLogLines: number;
  sentLines: number;
  totalSentLineLength: number;
  logLimitHit: boolean;
  timers: Map<string, number>;
};

export function defaultConsoleState(
  maxLogLines: number = DEFAULT_MAX_LOG_LINES,
): ConsoleState {
  return {
    maxLogLines,
    sentLines: 0,
    totalSentLineLength: 0,
    logLimitHit: false,
    timers: new Map(),
  };
}

export function setupConsole(responseStream: Writable) {
  // TODO(presley): For some reason capturing stdout and stderr doesn't work in
  // AWS Lambda. Not sure if it is async issue or AWS does something weird where
  // they patch node:console Console object. For now we will will throw away the
  // stdout and stderr and override a few methods directly.
  const lineBuffer = new Writable({
    construct: (callback) => {
      callback();
    },
    write: (chunk, encoding, callback) => {
      //this.consoleBuffer.push(chunk.toString());
      callback();
    },
  });
  const devConsole = new Console({
    stdout: lineBuffer,
    stderr: lineBuffer,
  });

  // TODO: This code is copy & pasted from setup.ts in v8. We should
  // probably unify it at some points.
  const consoleState = globalConsoleState.getStore()!;
  function consoleMessage(level: string, ...args: any[]) {
    // TODO: Support string substitution.
    // TODO: Implement the rest of the Console API.

    const serializedArgs = args.map((e: any) =>
      inspect(e, {
        // Our entire log line can't be more than 32768 bytes (MAX_LOG_LINE_LENGTH) so
        // keep string in here to no more than 32768 UTF-16 code units, and let
        // the backend truncate the whole log line if it is too long.
        maxStringLength: 32768,
        customInspect: true,
      }),
    );

    let messages = serializedArgs;
    // Requirements:
    // - 6MB limit on AWS lambda response size, so only collect
    //   maximum 2MB of logs, one ~million UTF16 code units (UTF16
    //   code unit is 2 bytes).
    // - at most `consoleState.maxLogLines` lines, the last of which is the
    //   overflow line.
    if (consoleState.logLimitHit === true) {
      return;
    }
    const totalMessageLength =
      messages.reduce((acc, current) => acc + current.length + 1, 0) - 1;
    if (consoleState.totalSentLineLength + totalMessageLength > 1_048_576) {
      level = "ERROR";
      messages = [
        "Log overflow (maximum 1M characters). Remaining log lines omitted.",
      ];
      consoleState.logLimitHit = true;
    } else if (consoleState.sentLines >= consoleState.maxLogLines - 1) {
      level = "ERROR";
      messages = [
        `Log overflow (maximum ${consoleState.maxLogLines}). Remaining log lines omitted.`,
      ];
      consoleState.logLimitHit = true;
    }
    responseStream.write(
      JSON.stringify({
        kind: "LogLine",
        data: {
          messages,
          isTruncated: false,
          timestamp: Date.now(),
          level,
        },
      }) + "\n",
    );
    consoleState.totalSentLineLength += totalMessageLength;
    consoleState.sentLines += 1;
  }

  devConsole.debug = function (...args) {
    consoleMessage("DEBUG", ...args);
  };
  devConsole.error = function (...args) {
    consoleMessage("ERROR", ...args);
  };
  devConsole.info = function (...args) {
    consoleMessage("INFO", ...args);
  };
  devConsole.log = function (...args) {
    consoleMessage("LOG", ...args);
  };
  devConsole.warn = function (...args) {
    consoleMessage("WARN", ...args);
  };
  devConsole.time = function (label: unknown) {
    const labelStr = toString(label, "default");
    if (consoleState.timers.has(labelStr)) {
      consoleMessage("WARN", `Timer '${labelStr}' already exists`);
    } else {
      consoleState.timers.set(labelStr, Date.now());
    }
  };
  devConsole.timeLog = function (label: unknown, ...args: any[]) {
    const labelStr = toString(label, "default");
    const time = consoleState.timers.get(labelStr);
    if (time === undefined) {
      consoleMessage("WARN", `Timer '${labelStr}' does not exist`);
    } else {
      const duration = Date.now() - time;
      consoleMessage("INFO", `${labelStr}: ${duration}ms`, ...args);
    }
  };
  devConsole.timeEnd = function (label: unknown) {
    const labelStr = toString(label, "default");
    const time = consoleState.timers.get(labelStr);
    if (time === undefined) {
      consoleMessage("WARN", `Timer '${labelStr}' does not exist`);
    } else {
      const duration = Date.now() - time;
      consoleState.timers.delete(labelStr);
      consoleMessage("INFO", `${labelStr}: ${duration}ms`);
    }
  };
  return devConsole;
}
