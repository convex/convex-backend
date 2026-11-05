// Docusaurus picks the Prism theme in JS from the color mode, which it only
// reads after hydration, so server-rendered code blocks carry the light
// theme's inline colors and flash on dark pages until React loads. This plugin
// emits both themes as CSS keyed on `html[data-theme]`, which Docusaurus sets
// in a blocking script before first paint. Pair it with an empty
// `themeConfig.prism.theme` so prism-react-renderer renders no inline colors.

function kebabCase(property) {
  return property.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
}

function declarations(style) {
  return Object.entries(style)
    .map(([property, value]) => `${kebabCase(property)}: ${value};`)
    .join(" ");
}

function themeToCss(colorMode, theme) {
  const root = `[data-theme="${colorMode}"]`;
  const rules = [
    `${root} { --prism-color: ${theme.plain.color}; --prism-background-color: ${theme.plain.backgroundColor}; }`,
  ];
  for (const { types, style, languages } of theme.styles) {
    if (languages) {
      throw new Error("Language-specific Prism styles are not supported");
    }
    const selector = types.map((type) => `${root} .token.${type}`).join(", ");
    rules.push(`${selector} { ${declarations(style)} }`);
  }
  return rules.join("\n");
}

function prismThemeCssPlugin(_context, { lightTheme, darkTheme }) {
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
