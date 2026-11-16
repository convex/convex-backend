// Docusaurus picks the Prism theme in JS from the color mode, which it only
// reads after hydration, so server-rendered code blocks carry the light
// theme's inline colors and flash on dark pages until React loads. This plugin
// lets the page pick the colors in CSS instead, from `html[data-theme]`, which
// Docusaurus sets in a blocking script before first paint.
//
// A token can have more than one type (for example `tag script keyword` for
// JavaScript inside a JSX attribute), and prism-react-renderer merges the
// styles of all its types in order, so the innermost type wins. Plain
// `.token.<type>` rules cannot express that order. Thus `dualModePrismTheme`
// makes a theme whose styles set one custom property per color mode and CSS
// property (`--prism-light-color`, `--prism-dark-font-style`, ...).
// prism-react-renderer still merges them per token, and the CSS of this plugin
// only reads the custom properties of the current color mode. Use
// `dualModePrismTheme` as `themeConfig.prism.theme` with the same two themes
// that you give to this plugin.

function kebabCase(property) {
  return property.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
}

function customProperty(colorMode, property) {
  return `--prism-${colorMode}-${kebabCase(property)}`;
}

function assertNoLanguages(theme) {
  for (const { languages } of theme.styles) {
    if (languages) {
      throw new Error("Language-specific Prism styles are not supported");
    }
  }
}

function dualModePrismTheme(lightTheme, darkTheme) {
  assertNoLanguages(lightTheme);
  assertNoLanguages(darkTheme);
  const styles = [];
  for (const [colorMode, theme] of [
    ["light", lightTheme],
    ["dark", darkTheme],
  ]) {
    for (const { types, style } of theme.styles) {
      styles.push({
        types,
        style: Object.fromEntries(
          Object.entries(style).map(([property, value]) => [
            customProperty(colorMode, property),
            value,
          ]),
        ),
      });
    }
  }
  // The colors of the code block itself come from the CSS of the plugin.
  return { plain: {}, styles };
}

function themeToCss(colorMode, theme) {
  const root = `[data-theme="${colorMode}"]`;
  const properties = new Set(
    theme.styles.flatMap(({ style }) => Object.keys(style)),
  );
  // When no type of the token sets the custom property, the declaration is
  // invalid at computed-value time, so the token keeps the plain style.
  const declarations = [...properties]
    .map(
      (property) =>
        `${kebabCase(property)}: var(${customProperty(colorMode, property)});`,
    )
    .join(" ");
  return [
    `${root} { --prism-color: ${theme.plain.color}; --prism-background-color: ${theme.plain.backgroundColor}; }`,
    `${root} .prism-code .token { ${declarations} }`,
  ].join("\n");
}

function prismThemeCssPlugin(_context, { lightTheme, darkTheme }) {
  assertNoLanguages(lightTheme);
  assertNoLanguages(darkTheme);
  const css = [
    themeToCss("light", lightTheme),
    themeToCss("dark", darkTheme),
  ].join("\n");
  return {
    name: "prism-theme-css",
    injectHtmlTags() {
      return {
        headTags: [{ tagName: "style", innerHTML: css }],
      };
    },
  };
}

module.exports = prismThemeCssPlugin;
module.exports.dualModePrismTheme = dualModePrismTheme;
