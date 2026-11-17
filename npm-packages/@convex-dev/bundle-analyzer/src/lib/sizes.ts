/** Attributed code and source map bytes, including their combined size. */
export type Sizes = {
  bytes: number;
  codeBytes: number;
  sourceMapBytes: number;
};

export function sizes(codeBytes = 0, sourceMapBytes = 0): Sizes {
  return { bytes: codeBytes + sourceMapBytes, codeBytes, sourceMapBytes };
}

export function sumSizes(values: Iterable<Sizes>): Sizes {
  let codeBytes = 0;
  let sourceMapBytes = 0;
  for (const value of values) {
    codeBytes += value.codeBytes;
    sourceMapBytes += value.sourceMapBytes;
  }
  return sizes(codeBytes, sourceMapBytes);
}
