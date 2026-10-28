import { withBrowser } from "./common.js";
import { loginToDashboard } from "./dashboardHelpers.js";

withBrowser(async (page) => {
  await loginToDashboard(page);

  // Open the dev deployment page (it's nice to test the dev
  // deployment page loads)
  await page.locator("a::-p-text(Development)").click();

  // Open the deployment selector
  await page.locator("#select-deployment").click();

  // Open the production page
  await page.locator("a::-p-text(Production)").click();

  // Make sure we're actually provisioning
  await page.waitForSelector("::-p-text(Provisioning your)");

  // See the Health page
  await page.waitForSelector("::-p-text(Health)");
}).catch((error) => {
  console.error(error);
  process.exit(1);
});
