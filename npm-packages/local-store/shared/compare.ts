import { IndexRangeBounds, Key } from "./types";
import { compareValues, Value } from "convex/values";

export function compareKeys(key1: Key, key2: Key): number {
  const result = _compareKeys(key1, key2);
  // onsole.log("compareKeys", key1, key2, result);
  return result;
}

function getValueAtIndex(
  v: Value[],
  index: number,
): { kind: "found"; value: Value } | undefined {
  if (index >= v.length) {
    return undefined;
  }
  return { kind: "found", value: v[index] };
}

function compareDanglingSuffix(
  shorterKeyKind: "exact" | "successor" | "predecessor",
  longerKeyKind: "exact" | "successor" | "predecessor",
  shorterKey: Key,
  longerKey: Key,
): number {
  if (shorterKeyKind === "exact" && longerKeyKind === "exact") {
    throw new Error(
      `Exact keys are not the same length:  ${JSON.stringify(
        shorterKey.value,
      )}, ${JSON.stringify(longerKey.value)}`,
    );
  }
  if (shorterKeyKind === "exact") {
    throw new Error(
      `Exact key is shorter than prefix: ${JSON.stringify(
        shorterKey.value,
      )}, ${JSON.stringify(longerKey.value)}`,
    );
  }
  if (shorterKeyKind === "predecessor" && longerKeyKind === "successor") {
    // successor is longer than predecessor, so it is bigger
    return -1;
  }
  if (shorterKeyKind === "successor" && longerKeyKind === "predecessor") {
    // successor is shorter than predecessor, so it is larger
    return 1;
  }
  if (shorterKeyKind === "predecessor" && longerKeyKind === "predecessor") {
    // predecessor of [2, 3] contains [2, 1] while predecessor of [2] doesn't, so longer predecessors are larger
    return -1;
  }
  if (shorterKeyKind === "successor" && longerKeyKind === "successor") {
    // successor of [2, 3] contains [2, 4] while successor of [2] doesn't, so longer successors are smaller
    return 1;
  }
  if (shorterKeyKind === "predecessor" && longerKeyKind === "exact") {
    return -1;
  }
  if (shorterKeyKind === "successor" && longerKeyKind === "exact") {
    return 1;
  }
  throw new Error(`Unexpected key kinds: ${shorterKeyKind}, ${longerKeyKind}`);
}

function _compareKeys(key1: Key, key2: Key): number {
  let i = 0;
  while (i < Math.max(key1.value.length, key2.value.length)) {
    const v1 = getValueAtIndex(key1.value as any, i);
    const v2 = getValueAtIndex(key2.value as any, i);
    if (v1 === undefined) {
      return compareDanglingSuffix(key1.kind, key2.kind, key1, key2);
    }
    if (v2 === undefined) {
      return -1 * compareDanglingSuffix(key2.kind, key1.kind, key2, key1);
    }
    const result = compareValues(v1.value, v2.value);
    if (result !== 0) {
      return result;
    }
    // if the prefixes are the same so far, keep going with the comparison
    i++;
  }

  if (key1.kind === key2.kind) {
    return 0;
  }

  // keys are the same length and values
  if (key1.kind === "exact") {
    if (key2.kind === "successor") {
      return -1;
    } else {
      return 1;
    }
  }
  if (key1.kind === "predecessor") {
    return -1;
  }
  if (key1.kind === "successor") {
    return 1;
  }
  throw new Error(`Unexpected key kind: ${key1.kind as any}`);
}

function testCompareKeys(
  key1: Key,
  key2: Key,
  expected: "firstBigger" | "secondBigger" | "equal",
) {
  const result = compareKeys(key1, key2);
  const expectedResult =
    expected === "firstBigger" ? 1 : expected === "secondBigger" ? -1 : 0;

  if (result !== expectedResult) {
    throw new Error(
      `compareKeys(${JSON.stringify(key1)}, ${JSON.stringify(
        key2,
      )}) = ${result}, expected ${expected}`,
    );
  }
}

export function testAllCompareKeys() {
  testCompareKeys(
    { kind: "exact", value: [2, 3] },
    { kind: "successor", value: [2] },
    "secondBigger",
  );
  testCompareKeys(
    { kind: "exact", value: [2] },
    { kind: "successor", value: [] },
    "secondBigger",
  );
  testCompareKeys(
    { kind: "predecessor", value: [2, 3] },
    { kind: "predecessor", value: [2] },
    "firstBigger",
  );
  testCompareKeys(
    { kind: "successor", value: [2, 3] },
    { kind: "successor", value: [2] },
    "secondBigger",
  );
  testCompareKeys(
    { kind: "successor", value: [2, 3] },
    { kind: "predecessor", value: [2, 3] },
    "firstBigger",
  );
  testCompareKeys(
    { kind: "successor", value: [2, 3] },
    { kind: "exact", value: [2, 3] },
    "firstBigger",
  );
  testCompareKeys(
    { kind: "predecessor", value: [2, 3] },
    { kind: "predecessor", value: [2, 3] },
    "equal",
  );
  testCompareKeys(
    { kind: "predecessor", value: [2] },
    { kind: "exact", value: [2, 3] },
    "secondBigger",
  );
}

export function minimalKey(indexRangeBounds: IndexRangeBounds): Key {
  if (indexRangeBounds.lowerBoundInclusive) {
    return { kind: "predecessor", value: indexRangeBounds.lowerBound };
  } else {
    return { kind: "successor", value: indexRangeBounds.lowerBound };
  }
}

export function maximalKey(indexRangeBounds: IndexRangeBounds): Key {
  if (indexRangeBounds.upperBoundInclusive) {
    return { kind: "successor", value: indexRangeBounds.upperBound };
  } else {
    return { kind: "predecessor", value: indexRangeBounds.upperBound };
  }
}
