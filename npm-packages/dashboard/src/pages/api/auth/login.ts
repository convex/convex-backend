import { NextApiRequest, NextApiResponse } from "next";
import { WorkOS } from "@workos-inc/node";
import { captureException } from "@sentry/nextjs";
import { safeReturnTo } from "lib/returnTo";
import { safeConnectionId, safeOrganizationId } from "lib/ssoLogin";
import { withWorkOSTimeout } from "server/workos";

export default async function handler(
  req: NextApiRequest,
  res: NextApiResponse,
) {
  if (req.method !== "GET") {
    return res.status(405).json({ error: "Method not allowed" });
  }

  const workos = new WorkOS(process.env.WORKOS_API_SECRET, {
    clientId: process.env.WORKOS_CLIENT_ID,
  });

  try {
    // Get the returnTo parameter from the query string
    const returnTo = safeReturnTo(req.query.returnTo, "/");
    const connectionId = safeConnectionId(req.query.connection_id);
    const organizationId = connectionId
      ? await connectionOrganizationId(workos, connectionId)
      : safeOrganizationId(req.query.organizationId);

    const authorizationUrl = workos.userManagement.getAuthorizationUrl({
      // Specify that we'd like AuthKit to handle the authentication flow
      // Can be GitHubOAuth, GoogleOAuth, etc.
      provider: "authkit",
      // Selects the organization during the AuthKit flow, which starts SSO
      // with its connection when the organization requires SSO.
      organizationId,
      // The callback endpoint that WorkOS will redirect to after a user authenticates
      redirectUri: `${
        process.env.WORKOS_REDIRECT_URI ||
        `https://${process.env.WORKOS_REDIRECT_URI_OVERRIDE}` ||
        ""
      }/api/auth/callback`,
      clientId: process.env.WORKOS_CLIENT_ID || "",
      // Pass the returnTo URL as state so it can be used in the callback
      state: returnTo,
    });

    // Redirect the user to the AuthKit sign-in page
    res.redirect(authorizationUrl);
  } catch (error) {
    console.error("Error generating authorization URL:", error);
    res.status(500).json({ error: "Internal server error" });
  }
}

// A connection belongs to exactly one organization.
async function connectionOrganizationId(
  workos: WorkOS,
  connectionId: string,
): Promise<string | undefined> {
  try {
    const connection = await withWorkOSTimeout(
      "getConnection",
      workos.sso.getConnection(connectionId),
    );
    return connection.organizationId;
  } catch (error) {
    // AuthKit without an organization still signs the user in, choosing the
    // connection by email domain.
    captureException(error);
    return undefined;
  }
}
