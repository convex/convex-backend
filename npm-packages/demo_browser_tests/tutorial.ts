import { assertDivWithContent, withBrowser } from "./common.js";
import { argv } from "node:process";

withBrowser(async (page) => {
  // Puppeteer's default navigation timeout is 30s, which can be too short on CI.
  page.setDefaultTimeout(120_000);
  page.setDefaultNavigationTimeout(120_000);
  await page.goto(`http://127.0.0.1:${argv[2]}`, {
    waitUntil: "domcontentloaded",
  });
  console.log("navigated to page");
  await page.waitForSelector(".badge");
  const myName = await page.evaluate(
    () => document.querySelector(".badge span")?.innerHTML,
  );
  if (myName === null) {
    throw "can't find user name";
  }
  if (myName?.indexOf("User ") !== 0) {
    console.log("found username", myName);
    throw "Username doesn't look like a random user name";
  }
  await assertDivWithContent(page, "ul", "");
  console.log("Initial content looks good (no messages)");
  await page.type("form input[placeholder]", "al pastor rocks");
  await page.click("form input[type='submit']");
  await assertDivWithContent(page, "span", `al pastor rocks`);
  console.log("Chat message was reflected in the message list, from us");
}).catch((error) => {
  console.error(error);
  process.exit(1);
});
