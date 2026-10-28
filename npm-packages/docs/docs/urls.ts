export function dashboardUrl(): string | undefined {
  if (window.location.hostname === "docs.convex.dev") {
    return undefined;
  }
  if (window.location.hostname === "localhost") {
    return `http://localhost:3000`;
  }
  return `https://${window.location.hostname}`;
}

export function gitUrl() {
  if (
    window.location.hostname === "docs.convex.dev" ||
    window.location.hostname === "localhost"
  ) {
    return "https://github.com/convex/convex-demos.git";
  }
  return `https://${window.location.hostname.replace(
    "-docs.",
    ".",
  )}/convex-demos.git`;
}
