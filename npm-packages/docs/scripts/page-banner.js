// Adds a Markdown banner to the top of a generated page, after its front
// matter.
function addPageBanner(contents, banner) {
  const frontMatter = /^---\n[\s\S]*?\n---\n/.exec(contents);
  const splitAt = frontMatter ? frontMatter[0].length : 0;
  return (
    contents.slice(0, splitAt) +
    "\n" +
    banner +
    "\n\n" +
    contents.slice(splitAt)
  );
}

module.exports = { addPageBanner };
