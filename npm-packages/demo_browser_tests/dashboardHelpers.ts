import { argv } from "node:process";
import { Page } from "puppeteer";
import { noteBrowserEvent } from "./common.js";

export const DASHBOARD_URL = "http://localhost:6789";

// Per-attempt budget for reaching the hosted login form. The first attempt also
// pays for Next.js compiling the route on demand, so it keeps the full timeout;
// retries hit a warm server.
const LOGIN_FORM_ATTEMPT_TIMEOUTS_MS = [60000, 20000, 20000];

// Navigate to the dashboard and wait for AuthKit's hosted login form.
// Retry on failure: AuthKit sometimes paints only its background and never
// renders the form; a fresh navigation clears that blank page.
async function gotoLoginForm(page: Page, path: string) {
  const attempts = LOGIN_FORM_ATTEMPT_TIMEOUTS_MS.length;
  for (const [attempt, timeout] of LOGIN_FORM_ATTEMPT_TIMEOUTS_MS.entries()) {
    try {
      await page.goto(DASHBOARD_URL + path, { waitUntil: "networkidle0" });
      await page.waitForSelector('input[name="email"]', {
        visible: true,
        timeout,
      });
      if (attempt > 0) {
        // The retry salvaged a run that used to fail outright. Counting these is
        // the only way to tell "AuthKit never blanked" from "it blanked and we
        // recovered", so record it where a passing test can't swallow it.
        noteBrowserEvent(
          `login-form-recovered after ${attempt} blank render(s), ` +
            `on attempt ${attempt + 1}/${attempts}`,
        );
      }
      return;
    } catch (error) {
      const message =
        `login form did not render at ${page.url()} ` +
        `(attempt ${attempt + 1}/${attempts}): ${error}`;
      console.error(message);
      noteBrowserEvent(`login-form-blank ${message}`);
      if (attempt === attempts - 1) {
        noteBrowserEvent("login-form-gave-up");
        throw error;
      }
    }
  }
}

// Activate a control from the keyboard. AuthKit's hosted UI renders layouts
// where a synthetic mouse click never reaches its buttons: hit testing at the
// center of the button's own bounding box lands on <html>, so the click is
// swallowed and the form is never submitted.
async function pressEnterOn(page: Page, selector: string) {
  const element = await page.waitForSelector(selector, { visible: true });
  await element!.focus();
  await page.keyboard.press("Enter");
}

type SignInOutcome = "signed-in" | "passkey-prompt" | `error: ${string}`;

// AuthKit submits its forms client-side, so signing in produces no navigation
// until it hands back to the dashboard. Wait for the outcome itself: landing
// off the AuthKit host, the passkey interstitial, or a rejected sign-in.
async function waitForSignInOutcome(
  page: Page,
  authKitHost: string,
): Promise<SignInOutcome> {
  const outcome = await page.waitForFunction(
    (host: string) => {
      if (window.location.host !== host) {
        return "signed-in";
      }
      const error = document.querySelector('[data-type="error"]');
      if (error) {
        return `error: ${error.textContent?.trim()}`;
      }
      const skipPasskey = [...document.querySelectorAll("button")].some(
        (button) => button.textContent?.includes("Skip for now"),
      );
      return skipPasskey ? "passkey-prompt" : false;
    },
    {},
    authKitHost,
  );
  return (await outcome.jsonValue()) as SignInOutcome;
}

export async function loginToDashboard(page: Page, path: string = "") {
  // We end up building large sections of code here, so increase the default
  // timeouts to reduce flakes on CI.
  page.setDefaultTimeout(60000);
  page.setDefaultNavigationTimeout(60000);

  await gotoLoginForm(page, path);
  await page.type(`input[name="email"]`, argv[2]);
  await pressEnterOn(page, 'input[name="email"]');

  await page.waitForSelector('input[name="password"]', { visible: true });
  await page.type(`input[name="password"]`, argv[3]);

  const authKitHost = new URL(page.url()).host;
  await pressEnterOn(page, 'input[name="password"]');

  let outcome = await waitForSignInOutcome(page, authKitHost);
  if (outcome === "passkey-prompt") {
    // The headless browser can't create passkeys, so decline the enrollment
    // interstitial ("Create a passkey for faster and more secure sign in").
    await pressEnterOn(page, "button::-p-text(Skip for now)");
    outcome = await waitForSignInOutcome(page, authKitHost);
  }
  if (outcome !== "signed-in") {
    throw new Error(`AuthKit rejected the sign-in: ${outcome}`);
  }
}
