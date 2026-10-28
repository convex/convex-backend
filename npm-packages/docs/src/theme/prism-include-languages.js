/**
 * Swizzle (wrap) of @docusaurus/theme-classic's prism-include-languages.
 *
 * We call the original implementation — which loads the grammars listed in
 * `themeConfig.prism.additionalLanguages` from prismjs core — and then register
 * `svelte`, whose grammar ships in the separate `prism-svelte` package rather
 * than in prismjs core. This enables ```svelte code blocks to highlight.
 *
 * Wrapping (rather than ejecting) keeps the `prismjs/components/*` requires
 * inside theme-classic, where prismjs is a dependency, so they still resolve.
 * We import the original from its concrete lib path because the `@theme-init`
 * alias isn't registered for this component in this Docusaurus version.
 */

import prismIncludeLanguages from "@docusaurus/theme-classic/lib/theme/prism-include-languages";

export default function prismIncludeLanguagesWrapper(PrismObject) {
  // Load the core/additional languages exactly as Docusaurus normally would.
  prismIncludeLanguages(PrismObject);

  // prism-svelte augments the Prism instance mounted on globalThis, so mount
  // PrismObject, register the grammar, then unmount to avoid polluting globals.
  globalThis.Prism = PrismObject;
  require("prism-svelte");
  delete globalThis.Prism;
}
