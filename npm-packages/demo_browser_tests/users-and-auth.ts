import { argv } from "node:process";
import { withBrowser } from "./common.js";

withBrowser(async (page) => {
  await page.goto(`http://localhost:${argv[2]}`);
  console.log("opened demo app");

  console.log("clicking login");

  await page.locator("button").click();

  await page.waitForNavigation();

  console.log("navigated to auth0 sign in screen");

  await page.locator("input#username").fill("jamie@convex.dev");
  await page.locator("input#password").fill("@9pFVGcmCJvHMCP*ti3QPg64");

  await page
    .locator("button[name='action']:not([aria-hidden])::-p-text(Continue)")
    .click();

  await page.waitForNavigation();

  console.log("navigated back to demo app");

  await page.waitForSelector("::-p-text(Logged in as jamie@convex.dev)");
  console.log("correct user is logged in!");
}).catch((error) => {
  console.error(error);
  process.exit(1);
});
