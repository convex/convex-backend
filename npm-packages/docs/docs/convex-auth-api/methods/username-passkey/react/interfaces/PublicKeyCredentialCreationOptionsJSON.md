---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: PublicKeyCredentialCreationOptionsJSON
---

import Link from "@docusaurus/Link";

A variant of PublicKeyCredentialCreationOptions suitable for JSON transmission
to the browser to (eventually) get passed into navigator.credentials.create(...)
in the browser.

This should eventually get replaced with official TypeScript DOM types when
WebAuthn L3 types eventually make it into the language:

https://w3c.github.io/webauthn/#dictdef-publickeycredentialcreationoptionsjson

## Properties

| Property                                                                | Type                                   |
| ----------------------------------------------------------------------- | -------------------------------------- |
| <Link id="property-attestation" /> `attestation?`                       | `AttestationConveyancePreference`      |
| <Link id="property-attestationformats" /> `attestationFormats?`         | `AttestationFormat`[]                  |
| <Link id="property-authenticatorselection" /> `authenticatorSelection?` | `AuthenticatorSelectionCriteria`       |
| <Link id="property-challenge" /> `challenge`                            | `string`                               |
| <Link id="property-excludecredentials" /> `excludeCredentials?`         | `PublicKeyCredentialDescriptorJSON`[]  |
| <Link id="property-extensions" /> `extensions?`                         | `AuthenticationExtensionsClientInputs` |
| <Link id="property-hints" /> `hints?`                                   | `PublicKeyCredentialHint`[]            |
| <Link id="property-pubkeycredparams" /> `pubKeyCredParams`              | `PublicKeyCredentialParameters`[]      |
| <Link id="property-rp" /> `rp`                                          | `PublicKeyCredentialRpEntity`          |
| <Link id="property-timeout" /> `timeout?`                               | `number`                               |
| <Link id="property-user" /> `user`                                      | `PublicKeyCredentialUserEntityJSON`    |
