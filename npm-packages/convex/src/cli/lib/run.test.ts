import chalk from "chalk";
import { afterEach, describe, expect, test } from "vitest";
import { highlightJson } from "./run.js";

const stripAnsi = (value: string) => value.replace(/\x1b\[[0-9;]*m/g, "");
const originalChalkLevel = chalk.level;

describe("highlightJson", () => {
  afterEach(() => {
    chalk.level = originalChalkLevel;
  });

  test("preserves valid JSON after ANSI codes are stripped", () => {
    chalk.level = 1;
    const value = {
      key: "value",
      count: 3,
      negative: -2.5e3,
      active: true,
      missing: null,
      nested: ["item", false],
      escaped: 'a"b',
    };

    const highlighted = highlightJson(JSON.stringify(value, null, 2));

    expect(JSON.parse(stripAnsi(highlighted))).toEqual(value);
  });

  test("adds ANSI color codes for JSON tokens", () => {
    chalk.level = 1;
    const value = {
      key: "value",
      count: 3,
      active: true,
      missing: null,
    };

    const highlighted = highlightJson(JSON.stringify(value, null, 2));

    expect(highlighted).toContain("\x1b[");
  });
});
