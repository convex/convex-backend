import input from "@inquirer/input";
import select from "@inquirer/select";
import search from "@inquirer/search";
import confirm from "@inquirer/confirm";
import { Context } from "../../../bundler/context.js";
import { logOutput } from "../../../bundler/log.js";

/**
 * Handle ExitPromptError thrown by @inquirer/* packages when the user
 * presses Ctrl+C. Instead of printing an ugly stack trace, exit cleanly.
 * For unexpected errors, use ctx.crash to report to Sentry.
 */
function handlePromptError(ctx: Context) {
  return async (error: unknown): Promise<never> => {
    if (error instanceof Error && error.name === "ExitPromptError") {
      // User pressed Ctrl+C — exit silently with code 130 (standard for SIGINT)
      process.exit(130);
    }
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Unexpected prompt error: ${String(error)}`,
      errForSentry: error instanceof Error ? error : undefined,
    });
  };
}

export const promptString = async (
  ctx: Context,
  options: {
    message: string;
    default?: string;
    validate?: (value: string) => boolean | string | Promise<string | boolean>;
  },
): Promise<string> => {
  if (process.stdin.isTTY) {
    return input({
      message: options.message,
      ...(options.default !== undefined ? { default: options.default } : {}),
      ...(options.validate !== undefined ? { validate: options.validate } : {}),
    }).catch(handlePromptError(ctx));
  } else {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Cannot prompt for input in non-interactive terminals. (${options.message})`,
    });
  }
};

export const promptSecret = async (
  ctx: Context,
  options: {
    message: string;
  },
): Promise<string> => {
  if (process.stdin.isTTY) {
    return input({
      message: options.message,
      transformer: (val, { isFinal }) =>
        isFinal ? "*".repeat(val.length) : val,
    }).catch(handlePromptError(ctx));
  } else {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Cannot prompt for input in non-interactive terminals. (${options.message})`,
    });
  }
};

export const promptOptions = async <V>(
  ctx: Context,
  options: {
    message: string;
    choices: Array<{ name: string; value: V }>;
    default?: V;
    prefix?: string;
    suffix?: string;
  },
): Promise<V> => {
  if (process.stdin.isTTY) {
    return select<V>({
      message: options.message + (options.suffix ?? ""),
      choices: options.choices,
      ...(options.default !== undefined ? { default: options.default } : {}),
      ...(options.prefix !== undefined
        ? { theme: { prefix: options.prefix } }
        : {}),
    }).catch(handlePromptError(ctx));
  } else {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Cannot prompt for input in non-interactive terminals. (${options.message})`,
    });
  }
};

export const promptSearch = async <V>(
  ctx: Context,
  options: {
    message: string;
    choices: Array<{ name: string; value: V }>;
    default?: V;
  },
): Promise<V> => {
  if (process.stdin.isTTY) {
    return search<V>({
      message: options.message,
      ...(options.default !== undefined ? { default: options.default } : {}),
      source: (input: string | undefined) => {
        if (!input) return options.choices;
        const term = input.toLowerCase();
        return options.choices.filter((c) =>
          c.name.toLowerCase().includes(term),
        );
      },
    }).catch(handlePromptError(ctx));
  } else {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Cannot prompt for input in non-interactive terminals. (${options.message})`,
    });
  }
};

export const promptYesNo = async (
  ctx: Context,
  options: {
    message: string;
    default?: boolean;
    prefix?: string;
    nonInteractiveError?: string;
  },
): Promise<boolean> => {
  if (process.stdin.isTTY) {
    return confirm({
      message: options.message,
      ...(options.default !== undefined ? { default: options.default } : {}),
      ...(options.prefix !== undefined
        ? { theme: { prefix: options.prefix } }
        : {}),
    }).catch(handlePromptError(ctx));
  } else {
    logOutput(options.message);
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        options.nonInteractiveError ??
        `Cannot prompt for input in non-interactive terminals. (${options.message})`,
    });
  }
};
