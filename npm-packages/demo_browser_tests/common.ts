import puppeteer, { type Browser, type Page } from "puppeteer";
import * as path from "node:path";
import * as fs from "node:fs";

// Record a notable event to a file CI archives (integration.yml uploads
// smoke/test_tempdir/*.log), because console output alone does not survive a
// PASSING test: pytest captures this process's stdout/stderr at the fd level and
// prints it only for tests that fail. A retry that works is exactly the case we
// most want to count, so it must not depend on the test going red.
export function noteBrowserEvent(message: string) {
  try {
    const outDir = process.env.SCREENSHOT_DIR || ".";
    const testName = path.basename(process.argv[1] || "unknown", ".js");
    fs.mkdirSync(outDir, { recursive: true });
    // O_APPEND keeps single short lines intact when xdist workers interleave.
    fs.appendFileSync(
      path.join(outDir, "browser-events.log"),
      `${new Date().toISOString()} ${testName} ${message}\n`,
    );
  } catch (error) {
    console.error("Failed to record browser event:", error);
  }
}

export async function withBrowser(
  testFn: (page: Page, browser: Browser) => Promise<void>,
): Promise<void> {
  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();
  try {
    await testFn(page, browser);
  } catch (error) {
    try {
      const outDir = process.env.SCREENSHOT_DIR || ".";
      const testName = path.basename(process.argv[1] || "unknown", ".js");
      const prefix = `${testName}-failure-${Date.now()}`;
      fs.mkdirSync(outDir, { recursive: true });

      // A screenshot alone can't tell "the page we wanted rendered nothing"
      // apart from "we were on a different page than we thought", which is the
      // difference between a third-party outage and a bug here.
      console.error(`Failed on ${page.url()} (title: ${await page.title()})`);

      const screenshot = path.join(outDir, `${prefix}.png`);
      await page.screenshot({ path: screenshot, fullPage: true });
      console.error(`Screenshot saved to: ${screenshot}`);

      const html = path.join(outDir, `${prefix}.html`);
      fs.writeFileSync(html, await page.content());
      console.error(`Page HTML saved to: ${html}`);
    } catch (diagnosticError) {
      console.error("Failed to capture failure diagnostics:", diagnosticError);
    }
    throw error;
  } finally {
    await page.close();
    await browser.close();
  }
}

export const assertDivWithContent = async (
  page: Page,
  selector: string,
  innerText: string,
) => {
  // The first argument to `page.waitForFunction()` is not a closure so closed
  // over variables need to be passed explicitly.
  await page.waitForFunction(
    (selector: string, innerText: string) => {
      const divs = [...document.querySelectorAll(selector)] as HTMLDivElement[];
      return divs.some((div: HTMLDivElement) => div.innerText === innerText);
    },
    {},
    selector,
    innerText,
  );
};

export const sleep = (durationMs: number) =>
  new Promise((r) => setTimeout(r, durationMs));
