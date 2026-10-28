import { DASHBOARD_URL, loginToDashboard } from "./dashboardHelpers.js";
import { sleep, withBrowser } from "./common.js";
import assert from "node:assert/strict";

withBrowser(async (page) => {
  await loginToDashboard(page);

  // Check that we have two projects
  await page.waitForSelector("a::-p-text(Development)");
  assert.equal(
    (await page.$$("a::-p-text(Development)")).length,
    2,
    "Expected two project cards with dev deployment links on home page",
  );

  // Open the first created project (they are sorted by creation time desc)
  // so that we have some history to go off of
  const devLinks = await page.$$("a::-p-text(Development)");
  await devLinks[devLinks.length - 1].click();
  await page.waitForSelector("#select-deployment");
  // If we don't wait here for a while, the page doesn't have a chance
  // to remember the last team/project/deployment
  await sleep(2000);

  /// Now test the redirects

  // Example team page redirect
  await page.goto(`${DASHBOARD_URL}/team/settings/members`);
  await page.waitForSelector("::-p-text(Invite Member)");

  // Example project page redirect
  await page.goto(`${DASHBOARD_URL}/project/settings`);
  await page.waitForSelector("::-p-text(Delete Project)");
  // check via URL that we're at the first created project,
  // which is the one we viewed. Its name comes from `test_dashboard.py`.
  assert.match(page.url(), /\/created-first\//);

  // Example deployment page redirect
  await page.goto(`${DASHBOARD_URL}/deployment/settings/pause-deployment`);
  await page.waitForSelector("::-p-text(This deployment is currently)");
  // check via URL that we're at the first created project,
  // which is the one we viewed. Its name comes from `test_dashboard.py`.
  assert.match(page.url(), /\/created-first\//);
}).catch((error) => {
  console.error(error);
  process.exit(1);
});
