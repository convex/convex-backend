import { argv } from "node:process";
import { sleep, withBrowser } from "./common.js";
import { loginToDashboard } from "./dashboardHelpers.js";

const main = async () => {
  // argv needed by loginToDashboard()
  if (argv.length < 4) {
    console.error("Usage: node dist/oauth_flow.js <username> <password>");
    console.error(
      "OAuth URL should be passed via OAUTH_URL environment variable",
    );
    process.exit(1);
  }

  const authUrl = process.env.OAUTH_URL;
  if (!authUrl) {
    console.error("OAUTH_URL environment variable not set");
    process.exit(1);
  }

  console.log(`Starting OAuth flow with URL: ${authUrl}`);

  await withBrowser(async (page) => {
    // We end up building large sections of code here, so increase the default
    // timeouts to reduce flakes on CI.
    page.setDefaultTimeout(60000);
    page.setDefaultNavigationTimeout(60000);

    console.log(`Original OAuth URL: ${authUrl}`);

    // First, log in to the dashboard normally
    await loginToDashboard(page);

    // After successful login, navigate to the OAuth authorization URL
    console.log(`Navigating to OAuth URL: ${authUrl}`);
    // `page.goto` throws net::ERR_ABORTED if the login redirect chain is
    // still in flight and supersedes our navigation, so retry thrown errors
    // as well as error statuses.
    const gotoAuthUrl = async () => {
      try {
        return await page.goto(authUrl);
      } catch (error) {
        console.log(`OAuth page navigation failed: ${error}`);
        return null;
      }
    };
    let gotoResponse = await gotoAuthUrl();
    let retries = 0;
    while ((!gotoResponse || gotoResponse.status() >= 400) && retries < 2) {
      console.log(
        `OAuth page returned status ${gotoResponse?.status()}, retrying (attempt ${retries + 1})...`,
      );
      await sleep(3000);
      gotoResponse = await gotoAuthUrl();
      retries++;
    }

    console.log(
      `Logged in and navigated to OAuth authorization page: ${page.url()}`,
    );

    // Wait for the OAuth authorization page to load and click authorize
    await page.waitForSelector("button::-p-text(Authorize)");
    console.log("Found Authorize button, clicking...");
    await page.locator("button::-p-text(Authorize)").click();
    console.log("Clicked Authorize button");

    // Wait a moment to see if anything happens
    await sleep(2000);
    console.log(`After clicking, current URL: ${page.url()}`);

    // Wait for redirect to callback URL
    console.log("Waiting for redirect to callback URL...");
    await page.waitForFunction(() =>
      window.location.href.includes("localhost:8080/callback"),
    );
    console.log(`OAuth flow completed! Redirected to: ${page.url()}`);
  });
};

main().catch((error) => {
  console.error("OAuth flow script failed:", error);
  process.exit(1);
});
