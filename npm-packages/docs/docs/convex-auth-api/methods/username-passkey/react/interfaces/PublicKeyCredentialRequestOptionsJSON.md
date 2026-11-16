---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: PublicKeyCredentialRequestOptionsJSON
---

import Link from "@docusaurus/Link";

A variant of PublicKeyCredentialRequestOptions suitable for JSON transmission to
the browser to (eventually) get passed into navigator.credentials.get(...) in
the browser.

## Properties

| Property                                                    | Type                                   |
| ----------------------------------------------------------- | -------------------------------------- |
| <Link id="property-allowcredentials" /> `allowCredentials?` | `PublicKeyCredentialDescriptorJSON`[]  |
| <Link id="property-challenge" /> `challenge`                | `string`                               |
| <Link id="property-extensions" /> `extensions?`             | `AuthenticationExtensionsClientInputs` |
| <Link id="property-hints" /> `hints?`                       | `PublicKeyCredentialHint`[]            |
| <Link id="property-rpid" /> `rpId?`                         | `string`                               |
| <Link id="property-timeout" /> `timeout?`                   | `number`                               |
| <Link id="property-userverification" /> `userVerification?` | `UserVerificationRequirement`          |
