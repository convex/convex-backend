import { expect, test } from "vitest";

import { compareValues } from "./compare.js";
import { compareUTF8 } from "./compare_utf8.js";

function encodeCodePoint(codePoint: number): number[] {
  // TextEncoder replaces lone surrogates with U+FFFD. Encode the numeric code
  // point directly to model the historical comparator for malformed UTF-16.
  if (codePoint < 0x80) {
    return [codePoint];
  }
  if (codePoint <= 0x07ff) {
    return [0xc0 | (codePoint >> 6), 0x80 | (codePoint & 0x3f)];
  }
  if (codePoint <= 0xffff) {
    return [
      0xe0 | (codePoint >> 12),
      0x80 | ((codePoint >> 6) & 0x3f),
      0x80 | (codePoint & 0x3f),
    ];
  }
  return [
    0xf0 | (codePoint >> 18),
    0x80 | ((codePoint >> 12) & 0x3f),
    0x80 | ((codePoint >> 6) & 0x3f),
    0x80 | (codePoint & 0x3f),
  ];
}

function referenceCompareUTF8(a: string, b: string): number {
  const length = Math.min(a.length, b.length);
  for (let i = 0; i < length; ) {
    const aCodePoint = a.codePointAt(i)!;
    const bCodePoint = b.codePointAt(i)!;
    if (aCodePoint !== bCodePoint) {
      const aBytes = encodeCodePoint(aCodePoint);
      const bBytes = encodeCodePoint(bCodePoint);
      const byteLength = Math.min(aBytes.length, bBytes.length);
      for (let byte = 0; byte < byteLength; byte++) {
        if (aBytes[byte] !== bBytes[byte]) {
          return aBytes[byte] - bBytes[byte];
        }
      }
      return aBytes.length - bBytes.length;
    }
    i += aCodePoint > 0xffff ? 2 : 1;
  }
  return a.length - b.length;
}

test("preserves exact historical numeric results", () => {
  const cases = [
    ["\u0000", "\u007f"],
    ["\u007f", "\u0080"],
    ["\u0080", "\u0100"],
    ["\u0080", "\u00bf"],
    ["\u07ff", "\u0800"],
    ["\u0800", "\u1000"],
    ["\u0800", "\u0840"],
    ["\u0800", "\u083f"],
    ["\uffff", "\u{10000}"],
    ["\u{10000}", "\u{40000}"],
    ["\u{10000}", "\u{11000}"],
    ["\u{10000}", "\u{10040}"],
    ["\u{10000}", "\u{1003f}"],
    ["same", "same\u0800"],
    ["same", "same\u{10000}"],
  ] as const;

  for (const [a, b] of cases) {
    const expected = referenceCompareUTF8(a, b);
    expect(
      compareUTF8(a, b),
      `${JSON.stringify(a)} < ${JSON.stringify(b)}`,
    ).toBe(expected);
    expect(
      compareUTF8(b, a),
      `${JSON.stringify(b)} > ${JSON.stringify(a)}`,
    ).toBe(-expected);
  }
});

test("preserves the numeric result exposed by compareValues", () => {
  expect(compareValues("\u0080", "\u0100")).toBe(-2);
  expect(compareValues("\u0800", "\u0840")).toBe(-1);
  expect(compareValues("\u{10000}", "\u{1003f}")).toBe(-63);
});

test("preserves comparison behavior for lone and paired surrogates", () => {
  const strings = [
    "\ud7ff",
    "\ud800",
    "\ud800A",
    "\ud800\udc00",
    "\udbff\udfff",
    "\udc00",
    "\udfff",
    "\ue000",
  ];

  for (const a of strings) {
    for (const b of strings) {
      expect(
        compareUTF8(a, b),
        `${JSON.stringify(a)}, ${JSON.stringify(b)}`,
      ).toBe(referenceCompareUTF8(a, b));
    }
  }
});

test("matches the reference for every adjacent code point", () => {
  let previous = String.fromCodePoint(0);
  for (let codePoint = 1; codePoint <= 0x10ffff; codePoint++) {
    const current = String.fromCodePoint(codePoint);
    const ascending = compareUTF8(previous, current);
    const expectedAscending = referenceCompareUTF8(previous, current);
    const descending = compareUTF8(current, previous);
    const expectedDescending = referenceCompareUTF8(current, previous);
    if (ascending !== expectedAscending || descending !== expectedDescending) {
      throw new Error(
        `Mismatch around U+${codePoint.toString(16)}: ` +
          `${ascending}, ${expectedAscending}, ${descending}, ${expectedDescending}`,
      );
    }
    previous = current;
  }
});

test("matches the reference for arbitrary UTF-16 strings", () => {
  let state = 0x2f6e2b1;
  const randomCodeUnit = () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state >>> 16;
  };
  const randomString = () => {
    const length = randomCodeUnit() % 12;
    let value = "";
    for (let i = 0; i < length; i++) {
      value += String.fromCharCode(randomCodeUnit());
    }
    return value;
  };

  for (let i = 0; i < 25_000; i++) {
    const a = randomString();
    const b = randomString();
    const actual = compareUTF8(a, b);
    const expected = referenceCompareUTF8(a, b);
    if (actual !== expected) {
      throw new Error(
        `Mismatch for ${JSON.stringify(a)}, ${JSON.stringify(b)}: ` +
          `${actual}, ${expected}`,
      );
    }
  }
});
