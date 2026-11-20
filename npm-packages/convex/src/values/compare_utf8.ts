/**
 * Derived from https://github.com/rocicorp/compare-utf8/tree/v0.1.1
 * (Apache Version 2.0, January 2004)
 */

/**
 * This is kept here instead of added as a dependency to avoid bundling issues.
 */

/**
 * Orders two JavaScript strings as if they were UTF-8 encoded byte arrays.
 * Returns the difference between the first unequal bytes, or the difference
 * between the UTF-16 lengths after all compared code points are equal. For
 * malformed UTF-16, lone surrogate code units are treated as three-byte code
 * points to preserve the historical behavior.
 *
 * @param {string} a
 * @param {string} b
 * @returns {number}
 */
export function compareUTF8(a: string, b: string): number {
  if (a === b) {
    return 0;
  }

  const aLength = a.length;
  const bLength = b.length;
  const length = Math.min(aLength, bLength);
  for (let i = 0; i < length; ) {
    const aCodeUnit = a.charCodeAt(i);
    const bCodeUnit = b.charCodeAt(i);
    if (aCodeUnit !== bCodeUnit) {
      // Code points below 0x80 are represented the same way in UTF-8 as in
      // UTF-16.
      if (aCodeUnit < 0x80 && bCodeUnit < 0x80) {
        return aCodeUnit - bCodeUnit;
      }
      const aCodePoint =
        aCodeUnit >= 0xd800 && aCodeUnit <= 0xdbff
          ? a.codePointAt(i)!
          : aCodeUnit;
      const bCodePoint =
        bCodeUnit >= 0xd800 && bCodeUnit <= 0xdbff
          ? b.codePointAt(i)!
          : bCodeUnit;
      return compareCodePointsAsUTF8(aCodePoint, bCodePoint);
    }

    if (aCodeUnit >= 0xd800 && aCodeUnit <= 0xdbff) {
      // Equal leading surrogates can still represent different code points
      // when only one string has a trailing surrogate.
      const aCodePoint = a.codePointAt(i)!;
      const bCodePoint = b.codePointAt(i)!;
      if (aCodePoint !== bCodePoint) {
        return compareCodePointsAsUTF8(aCodePoint, bCodePoint);
      }
      if (aCodePoint > 0xffff) {
        i += 2;
        continue;
      }
    }
    i++;
  }

  return aLength - bLength;
}

/**
 * Return the difference between the first unequal bytes in the UTF-8
 * encodings of two different code points.
 *
 * @param {number} a
 * @param {number} b
 * @returns {number}
 */
function compareCodePointsAsUTF8(a: number, b: number): number {
  const aByteLength = a < 0x80 ? 1 : a <= 0x07ff ? 2 : a <= 0xffff ? 3 : 4;
  const bByteLength = b < 0x80 ? 1 : b <= 0x07ff ? 2 : b <= 0xffff ? 3 : 4;

  if (aByteLength === bByteLength) {
    // Code points fit in 21 bits, so signed 32-bit bitwise coercion preserves
    // their values. UTF-8 stores those bits in six-bit groups. With equal byte
    // lengths, the encoding prefixes cancel, so the highest differing group
    // gives the first unequal byte without materializing either encoding.
    const shift = Math.floor((31 - Math.clz32(a ^ b)) / 6) * 6;
    return (a >> shift) - (b >> shift);
  }

  const aLeadingByte =
    aByteLength === 1
      ? a
      : aByteLength === 2
        ? 0xc0 | (a >> 6)
        : aByteLength === 3
          ? 0xe0 | (a >> 12)
          : 0xf0 | (a >> 18);
  const bLeadingByte =
    bByteLength === 1
      ? b
      : bByteLength === 2
        ? 0xc0 | (b >> 6)
        : bByteLength === 3
          ? 0xe0 | (b >> 12)
          : 0xf0 | (b >> 18);
  return aLeadingByte - bLeadingByte;
}
