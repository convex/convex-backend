import { Option } from "@commander-js/extra-typings";

export const EXTERNAL_PACKAGES_DOCS =
  "https://docs.convex.dev/functions/bundling#external-packages";

export const CODE_SIZE_LIMITS_DOCS =
  "https://docs.convex.dev/functions/bundling#code-size-limits";

/** Default `--limit` for the list commands. */
export const DEFAULT_LIMIT = 30;

/** Import chains shown per package copy or file when `--limit` isn't passed. */
export const DEFAULT_CHAINS = 3;

export function jsonOption() {
  return new Option("--json", "Print machine-readable JSON to stdout");
}
