import { useEffect } from "react";
import { captureMessage } from "@sentry/nextjs";

export type ProviderKind = "directory type" | "SSO connection type";

export type Provider = {
  label: string;
  iconSlug?: string;
  unmapped?: ProviderKind;
};

const DIRECTORY_PROVIDERS = new Map<string, Provider>([
  ["azure scim v2.0", { label: "Entra ID", iconSlug: "azure" }],
  ["bamboohr", { label: "BambooHR", iconSlug: "bamboo-hr" }],
  ["breathe hr", { label: "Breathe HR", iconSlug: "breathe-hr" }],
  ["cezanne hr", { label: "Cezanne HR", iconSlug: "cezanne-hr" }],
  ["cyberark scim v2.0", { label: "CyberArk", iconSlug: "cyberark" }],
  ["fourth hr", { label: "Fourth", iconSlug: "fourth" }],
  ["generic scim v2.0", { label: "Generic SCIM v2.0" }],
  ["gsuite directory", { label: "Google Workspace", iconSlug: "google-cloud" }],
  ["hibob", { label: "HiBob", iconSlug: "hibob" }],
  ["jump cloud scim v2.0", { label: "JumpCloud", iconSlug: "jumpcloud" }],
  ["okta scim v2.0", { label: "Okta", iconSlug: "okta" }],
  ["onelogin scim v2.0", { label: "OneLogin", iconSlug: "onelogin" }],
  ["people hr", { label: "Access People HR", iconSlug: "access-people-hr" }],
  ["personio", { label: "Personio", iconSlug: "personio" }],
  [
    "pingfederate scim v2.0",
    { label: "PingFederate", iconSlug: "ping-identity" },
  ],
  ["rippling scim v2.0", { label: "Rippling", iconSlug: "rippling" }],
  ["sftp", { label: "SFTP" }],
  ["sftp workday", { label: "Workday (SFTP)", iconSlug: "workday" }],
  ["workday", { label: "Workday", iconSlug: "workday" }],
]);

const CONNECTION_PROVIDERS = new Map<string, Provider>([
  ["ADFSSAML", { label: "ADFS SAML" }],
  ["AdpOidc", { label: "ADP OIDC", iconSlug: "adp" }],
  ["AppleOAuth", { label: "Apple OAuth", iconSlug: "apple" }],
  ["Auth0SAML", { label: "Auth0 SAML", iconSlug: "auth0" }],
  ["AzureSAML", { label: "Entra ID SAML", iconSlug: "azure" }],
  ["CasSAML", { label: "CAS SAML", iconSlug: "cas" }],
  ["ClassLinkSAML", { label: "ClassLink SAML", iconSlug: "classlink" }],
  ["CleverOIDC", { label: "Clever OIDC", iconSlug: "clever" }],
  ["CloudflareSAML", { label: "Cloudflare SAML", iconSlug: "cloudflare" }],
  ["CyberArkSAML", { label: "CyberArk SAML", iconSlug: "cyberark" }],
  ["DuoSAML", { label: "Duo SAML", iconSlug: "duo" }],
  ["EntraIdOIDC", { label: "Entra ID OIDC", iconSlug: "azure" }],
  ["GenericOIDC", { label: "Generic OIDC", iconSlug: "generic-oidc" }],
  ["GenericSAML", { label: "Generic SAML" }],
  ["GitHubOAuth", { label: "GitHub OAuth", iconSlug: "github" }],
  ["GoogleOAuth", { label: "Google OAuth", iconSlug: "google" }],
  ["GoogleSAML", { label: "Google Workspace SAML", iconSlug: "google-cloud" }],
  ["JumpCloudSAML", { label: "JumpCloud SAML", iconSlug: "jumpcloud" }],
  ["KeycloakSAML", { label: "Keycloak SAML", iconSlug: "keycloak" }],
  ["LastPassSAML", { label: "LastPass SAML", iconSlug: "lastpass" }],
  ["LoginGovOidc", { label: "Login.gov OIDC", iconSlug: "login-gov" }],
  ["MagicLink", { label: "Magic Link" }],
  ["MicrosoftOAuth", { label: "Microsoft OAuth", iconSlug: "microsoft" }],
  ["MiniOrangeSAML", { label: "miniOrange SAML", iconSlug: "miniorange" }],
  ["NetIqSAML", { label: "NetIQ SAML", iconSlug: "net-iq" }],
  ["OktaOIDC", { label: "Okta OIDC", iconSlug: "okta" }],
  ["OktaSAML", { label: "Okta SAML", iconSlug: "okta" }],
  ["OneLoginSAML", { label: "OneLogin SAML", iconSlug: "onelogin" }],
  ["OracleSAML", { label: "Oracle SAML", iconSlug: "oracle" }],
  [
    "PingFederateSAML",
    { label: "PingFederate SAML", iconSlug: "ping-identity" },
  ],
  ["PingOneSAML", { label: "PingOne SAML", iconSlug: "ping-identity" }],
  ["RipplingSAML", { label: "Rippling SAML", iconSlug: "rippling" }],
  ["SalesforceOAuth", { label: "Salesforce OAuth", iconSlug: "salesforce" }],
  ["SalesforceSAML", { label: "Salesforce SAML", iconSlug: "salesforce" }],
  [
    "ShibbolethGenericSAML",
    { label: "Shibboleth Generic SAML", iconSlug: "shibboleth" },
  ],
  ["ShibbolethSAML", { label: "Shibboleth SAML", iconSlug: "shibboleth" }],
  [
    "SimpleSamlPhpSAML",
    { label: "SimpleSAMLphp SAML", iconSlug: "simple-saml-php" },
  ],
  ["VMwareSAML", { label: "VMware SAML", iconSlug: "vmware" }],
]);

function lookup(
  providers: Map<string, Provider>,
  kind: ProviderKind,
  type: string,
): Provider {
  return providers.get(type) ?? { label: type, unmapped: kind };
}

export function directoryProvider(
  type: string | null | undefined,
): Provider | undefined {
  return type ? lookup(DIRECTORY_PROVIDERS, "directory type", type) : undefined;
}

export function connectionProvider(connectionType: string): Provider {
  return lookup(CONNECTION_PROVIDERS, "SSO connection type", connectionType);
}

export function providerIconUrl(iconSlug: string, mode: "light" | "dark") {
  return `https://cdn.workos.com/provider-icons/${mode}/${iconSlug}.svg`;
}

// Module-scoped so each provider is reported once per page load, however often
// the sheets that render it remount.
export const reportedUnmapped = new Set<string>();

export function useReportUnmappedProviders(
  providers: (Provider | undefined)[],
) {
  const unmapped = providers
    .flatMap((p) =>
      p?.unmapped ? [`${p.unmapped} ${JSON.stringify(p.label)}`] : [],
    )
    .join("\n");

  useEffect(() => {
    for (const provider of unmapped === "" ? [] : unmapped.split("\n")) {
      if (reportedUnmapped.has(provider)) {
        continue;
      }
      reportedUnmapped.add(provider);
      captureMessage(
        `Unmapped WorkOS ${provider}. It renders as its raw WorkOS type, with no icon, until it is added to providers.ts.`,
        "error",
      );
    }
  }, [unmapped]);
}
