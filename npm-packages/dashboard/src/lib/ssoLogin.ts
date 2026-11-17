function safeWorkOSId(prefix: string, value: unknown): string | undefined {
  return typeof value === "string" &&
    new RegExp(`^${prefix}_[0-9A-Za-z]+$`).test(value)
    ? value
    : undefined;
}

export function safeOrganizationId(value: unknown): string | undefined {
  return safeWorkOSId("org", value);
}

export function safeConnectionId(value: unknown): string | undefined {
  return safeWorkOSId("conn", value);
}

export function ssoLoginPath(organizationId: string, returnTo: string): string {
  const params = new URLSearchParams({ organizationId, returnTo });
  return `/api/auth/login?${params}`;
}
