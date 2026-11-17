// `@pierre/trees` and `@pierre/theming` only export ES modules, under the
// `import` condition that Jest's CommonJS resolution doesn't use.
module.exports = (request, options) =>
  options.defaultResolver(
    request,
    request.startsWith("@pierre/")
      ? { ...options, conditions: ["import", "default"] }
      : options,
  );
