// TypeDoc plugin for the API reference generated from the `convex` package.
//
// - Builds the "Defined in" links from `sourceLinkTemplate`, with `{path}`
//   relative to `sourceLinkRoot` and `{gitRevision}` from `gitRevision`.
//   TypeDoc's own `{path}` is relative to the package, and release builds
//   document a separate checkout of a release (see reference-source.js).
// - Adds `pageBanner` (Markdown) to the top of every page, after the front
//   matter.
//
// The `typedoc` module isn't a direct dependency of the docs package, so event
// names come from the classes of `app`'s components.

const path = require("path");
const { addPageBanner } = require("./page-banner.js");

exports.load = function (app) {
  app.options.addDeclaration({
    name: "sourceLinkRoot",
    help: "Absolute directory that `{path}` in sourceLinkTemplate is relative to.",
  });
  app.options.addDeclaration({
    name: "pageBanner",
    help: "Markdown added to the top of every page.",
  });

  // Runs after TypeDoc's own SourcePlugin, which fills `url` on
  // EVENT_RESOLVE_BEGIN.
  app.converter.on(app.converter.constructor.EVENT_RESOLVE_END, (context) => {
    const root = app.options.getValue("sourceLinkRoot");
    const template = app.options
      .getValue("sourceLinkTemplate")
      .replaceAll("{gitRevision}", app.options.getValue("gitRevision"));
    for (const reflection of Object.values(context.project.reflections)) {
      for (const source of reflection.sources ?? []) {
        const relativePath = path
          .relative(root, source.fullFileName)
          .split(path.sep)
          .join("/");
        source.url = template
          .replaceAll("{path}", relativePath)
          .replaceAll("{line}", String(source.line));
      }
    }
  });

  // The Docusaurus theme prepends front matter on the same event, before or
  // after this listener depending on registration order.
  app.renderer.on(app.renderer.constructor.EVENT_END_PAGE, (page) => {
    const banner = app.options.getValue("pageBanner");
    if (!banner || !page.contents) {
      return;
    }
    page.contents = addPageBanner(page.contents, banner);
  });
};
