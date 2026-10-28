import { assertDivWithContent, withBrowser } from "./common.js";

withBrowser(async (page) => {
  await page.goto("http://localhost:3000");
  page.setDefaultTimeout(5000);
  console.log("navigated to page");
  await assertDivWithContent(page, "div", "Here's the counter:");
  await assertDivWithContent(page, "div", "0");
  await assertDivWithContent(page, "button", "Add One!");
  console.log("Initial content looks good");

  await page.click("button");
  console.log("Clicked button");
  await assertDivWithContent(page, "div", "1");
  console.log("Counter incremented!");
  await page.click("button");
  console.log("Clicked button");
  await assertDivWithContent(page, "div", "2");
  console.log("Counter incremented again. It's all happening.");
}).catch((error) => {
  console.error(error);
  process.exit(1);
});
