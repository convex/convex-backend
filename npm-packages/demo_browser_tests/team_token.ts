import { argv } from "node:process";
import { withBrowser } from "./common.js";
import { loginToDashboard, DASHBOARD_URL } from "./dashboardHelpers.js";

const TEAM_NAME = "engineering-smoketests";

const main = async () => {
  // argv needed by loginToDashboard()
  if (argv.length < 4) {
    console.error("Usage: node dist/team_token.js <username> <password>");
    process.exit(1);
  }

  console.log(`Getting team token for team: ${TEAM_NAME}`);

  await withBrowser(async (page) => {
    // We end up building large sections of code here, so increase the default
    // timeouts to reduce flakes on CI.
    page.setDefaultTimeout(60000);
    page.setDefaultNavigationTimeout(60000);

    // First, log in to the dashboard normally
    await loginToDashboard(page);

    // Navigate to the team's access tokens page
    const accessTokensUrl = `${DASHBOARD_URL}/t/${TEAM_NAME}/settings/access-tokens`;
    console.log(`Navigating to: ${accessTokensUrl}`);
    await page.goto(accessTokensUrl);

    console.log(`Navigated to team access tokens page: ${page.url()}`);

    // Click the "Create token" button
    console.log("Looking for Create token button...");
    await page.waitForSelector("button::-p-text(Create Token)", {
      timeout: 30000,
    });
    console.log("Found Create token button, clicking...");
    await page.locator("button::-p-text(Create Token)").click();

    // Wait for the token name input field and fill it
    console.log("Waiting for token name input field...");
    await page.waitForSelector("input#tokenName", { timeout: 30000 });
    console.log("Found token name input, filling it...");
    const tokenName = "smoke-test-token";
    await page.locator("input#tokenName").fill(tokenName);

    // Submit the form (look for a submit button)
    console.log("Looking for submit button...");
    await page.waitForSelector("button[type='submit']", { timeout: 30000 });
    await page.locator("button[type='submit']").click();

    // Wait for the token to appear in the list
    console.log(`Waiting for token "${tokenName}" to appear...`);
    await page.waitForSelector(`::-p-text(${tokenName})`, { timeout: 30000 });
    console.log("Token created successfully!");

    // Find the "Show" button next to our token
    console.log("Looking for Show button...");
    await page.waitForSelector("button::-p-text(Show)", { timeout: 30000 });
    await page.locator("button::-p-text(Show)").click();

    // Wait for and extract the token value that starts with "eyJ"
    console.log("Waiting for token value to be revealed...");
    await page.waitForSelector("span::-p-text(eyJ)", { timeout: 30000 });

    const token = await page.$eval(
      "span::-p-text(eyJ)",
      (el: Element) => el.textContent,
    );

    if (!token || !token.startsWith("eyJ")) {
      throw new Error("Failed to extract valid token");
    }

    console.log(`Successfully extracted team token: ${token}`);
    console.log(token); // Output the token for the calling script to capture
  });
};

main().catch((error) => {
  console.error("Team token script failed:", error);
  process.exit(1);
});
