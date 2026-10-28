import { assertDivWithContent, withBrowser } from "./common.js";
import { argv } from "node:process";

withBrowser(async (page) => {
  await page.goto(`http://127.0.0.1:${argv[2]}`);
  console.log("navigated to page");
  await page.waitForSelector(".badge > span > input");
  await assertDivWithContent(page, "ul", "");
  console.log("Initial content looks good (no messages)");
  await page.type("form > input[placeholder]", "al pastor rocks");
  await page.click("input[type='submit']");
  await assertDivWithContent(page, "span", `al pastor rocks`);
  console.log("Chat message was reflected in the message list, from us");
}).catch((error) => {
  console.error(error);
  process.exit(1);
});
