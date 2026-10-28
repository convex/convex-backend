import { withBrowser } from "./common.js";
import { DASHBOARD_URL, loginToDashboard } from "./dashboardHelpers.js";

withBrowser(async (page) => {
  /// Test the redirects, even before we ever visit
  // a team, project or deployment page

  // After login we'll be redirected to an example
  // team page.
  await loginToDashboard(page, "/team/settings/members");
  await page.waitForSelector("::-p-text(Invite Member)");

  // Example project page redirect
  await page.goto(`${DASHBOARD_URL}/project/settings`);
  await page.waitForSelector("::-p-text(Delete Project)");

  // Example deployment page redirect
  await page.goto(`${DASHBOARD_URL}/deployment/settings/pause-deployment`);
  await page.waitForSelector("::-p-text(This deployment is currently)");
}).catch((error) => {
  console.error(error);
  process.exit(1);
});
