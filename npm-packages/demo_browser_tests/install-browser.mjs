// Make sure a browser puppeteer can launch is available, downloading one only
// if nothing has already provided it.
//
// The arm64 runner AMI bakes in a linux-arm64 chromium and exports
// PUPPETEER_EXECUTABLE_PATH for it, which puppeteer.launch() reads directly.
// There, downloading is not just redundant but harmful: Chrome for Testing
// publishes no linux-arm64 build, so `puppeteer browsers install chrome`
// unpacks the x86-64 archive under a linux_arm directory. That binary can't
// exec, and the shell reinterprets it as a script, reporting the very
// confusing `chrome: 1: Syntax error: ";" unexpected`.
//
// The x64 AMI sets no such variable and puppeteer's own download works there,
// so keying off the variable covers both.
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";

const provided = process.env.PUPPETEER_EXECUTABLE_PATH;

if (!provided) {
  execFileSync("puppeteer", ["browsers", "install", "chrome"], {
    stdio: "inherit",
    shell: true,
  });
} else if (existsSync(provided)) {
  console.log(`Using the browser at PUPPETEER_EXECUTABLE_PATH=${provided}`);
} else {
  console.error(
    `PUPPETEER_EXECUTABLE_PATH=${provided} does not exist. Point it at a ` +
      `browser, or unset it to download one (unavailable on linux-arm64).`,
  );
  process.exit(1);
}
