---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: AuthenticationResponseJSON
---

import Link from "@docusaurus/Link";

A slightly-modified AuthenticationCredential to simplify working with
ArrayBuffers that are Base64URL-encoded in the browser so that they can be sent
as JSON to the server.

https://w3c.github.io/webauthn/#dictdef-authenticationresponsejson

## Properties

| Property                                                                  | Type                                    |
| ------------------------------------------------------------------------- | --------------------------------------- |
| <Link id="property-authenticatorattachment" /> `authenticatorAttachment?` | `AuthenticatorAttachment`               |
| <Link id="property-clientextensionresults" /> `clientExtensionResults`    | `AuthenticationExtensionsClientOutputs` |
| <Link id="property-id" /> `id`                                            | `string`                                |
| <Link id="property-rawid" /> `rawId`                                      | `string`                                |
| <Link id="property-response" /> `response`                                | `AuthenticatorAssertionResponseJSON`    |
| <Link id="property-type" /> `type`                                        | `"public-key"`                          |
