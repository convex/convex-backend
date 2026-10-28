/* Swizzled wrapper around the OpenAPI theme's ResponseExamples.
 *
 * The real components pass through untouched, except for `ExampleFromSchema`:
 * upstream synthesizes an "Example (auto)" tab from the schema whenever an
 * operation declares no explicit example. That schema is already documented
 * right next to it, so the auto tab is just a confusing duplicate — we render
 * nothing instead, which drops the tab entirely (SchemaTabs filters out null
 * children). Explicit, spec-provided examples still render via ResponseExample /
 * ResponseExamples below. */
// @ts-expect-error -- @theme-original ships no bundled type declarations
export {
  ResponseExamples,
  ResponseExample,
  json2xml,
} from "@theme-original/ResponseExamples";

export const ExampleFromSchema = (_props?: {
  schema: any;
  mimeType: string;
}): null => null;
