# Demo Browser Tests

This directory contains a number of tests which use puppeteer to drive headless
chromium to make sure that the in-browser behavior of our demo apps actually
works!

## Getting a browser

`postinstall` downloads a Chrome for puppeteer, unless
`PUPPETEER_EXECUTABLE_PATH` already points at one. CI's arm64 runners set that
variable to a chromium baked into the AMI, because Chrome for Testing publishes
no linux-arm64 build — the download silently yields an x86-64 binary that can't
exec, reported as `chrome: 1: Syntax error: ";" unexpected`.

So on a linux-arm64 workstation, install a chromium and point at it:

```sh
export PUPPETEER_EXECUTABLE_PATH=/path/to/chromium
```

## Finding out what happened in CI

Failures write a screenshot and a page HTML dump next to the logs, and notable
events — a blank AuthKit login page, and whether re-navigating recovered it —
are appended to `browser-events.log`. All of it lands in the `smoke logs`
artifact of the run.

Grep that file for `login-form-recovered` to count flakes the retry absorbed,
and `login-form-gave-up` for the ones it couldn't. Console output can't be used
for this: pytest captures it and prints it only for tests that fail, so a
successful retry would leave no trace.

## Current Oddities + Limitations

1.  The selectors are a little funky, indirect, and dependent on our current
    demo code structure instead of, say, element ids. So therefore they're a bit
    fragile. Right now, this is preferrable to complicating our demo code with
    ids that aren't used for anything in-demo (and are only used for testing).
2.  `users-and-auth` uses authentication with auth0. Yes, we do test this, but
    it's with a test account that jamie has created at auth0 for this testing
    app specifically.

## Dashboard tests

There are dashboard tests here too!

## Platform (public management API) test

These too, using dashboard browser automation helpers code to go through an
OAuth flow to get a OAuth team token.
