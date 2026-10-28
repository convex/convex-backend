import { assertDivWithContent, withBrowser } from "./common.js";
import { argv } from "node:process";

withBrowser(async (page) => {
  await page.goto(`http://127.0.0.1:${argv[2]}`);
  console.log("navigated to page");
  await page.waitForSelector(".badge");
  const myName = await page.evaluate(
    () => document.querySelector(".badge span")?.textContent,
  );
  if (myName === null) {
    throw "can't find user name";
  }
  if (myName?.indexOf("User ") !== 0) {
    throw "Username doesn't look like a random user name";
  }

  await page.type("input[placeholder]", "mychannel");
  await page.click("input[type='submit']");

  // TODO Waiting for this "div.channel-box a" to be present fixes 1/20 times
  // flake in CI. This is probably a real race of some kind, but fixing it is
  // not a top priority.
  // https://linear.app/convex/issue/CX-1476/first-flakey-browser-integration-test
  await page.waitForSelector(".channel-box li");

  await page.type("input[placeholder]", "mychannel2");
  await page.click("input[type='submit']");
  await page.waitForSelector(".channel-box li:nth-child(2)");
  console.log("Created channel");
  await assertDivWithContent(page, ".channel-box li", "mychannel");
  await assertDivWithContent(page, ".channel-box li", "mychannel2");

  // First link == mychannel
  await page.click(".channel-box li");
  console.log("entered first channel");

  await page.waitForSelector(".chat-box ul");
  await assertDivWithContent(page, ".chat-box ul", "");
  console.log("Initial content looks good (no messages)");
  await page.type(".chat-box input[placeholder]", "al pastor rocks");
  await page.click(".chat-box input[type='submit']");
  // Read for "my name" and then the message
  await assertDivWithContent(page, "span", `al pastor rocks`);
  console.log("Okay, first channel has our message");

  // Second link == mychannel2
  await page.click(".channel-box li:nth-child(2)");
  console.log("entered second channel");
  // No messages
  await assertDivWithContent(page, ".chat-box ul", "");
  console.log("Second channel, still empty. yay!");
}).catch((error) => {
  console.error(error);
  process.exit(1);
});
