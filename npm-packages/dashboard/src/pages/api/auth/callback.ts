import { NextApiRequest, NextApiResponse } from "next";
import { WorkOS } from "@workos-inc/node";
import { captureException } from "@sentry/nextjs";
import { createSessionCookie, withWorkOSTimeout } from "server/workos";
import { safeReturnTo } from "lib/returnTo";

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

  // The authorization code returned by AuthKit
  const code = req.query.code as string;
  const state = req.query.state as string; // This contains our returnTo URL

  const { resource_id, path, url } = req.query;

  if (!code) {
    return res.status(400).send("No code provided");
  }

  try {
    const authenticateResponse = await withWorkOSTimeout(
      "authenticateWithCode",
      workos.userManagement.authenticateWithCode({
        clientId: process.env.WORKOS_CLIENT_ID || "",
        code,
        session: {
          sealSession: true,
          cookiePassword: process.env.WORKOS_COOKIE_PASSWORD,
        },
      }),
    );

    const { sealedSession, authenticationMethod } = authenticateResponse;

    if (!sealedSession) {
      return res
        .status(500)
        .json({ error: "No sealed session returned from WorkOS" });
    }

    res.setHeader("Set-Cookie", createSessionCookie(sealedSession));

    // `state` carries the `returnTo` we set in login.ts. Only honor it if it's
    // a safe same-origin path, and not an `/api` route (which would loop back
    // through auth).
    const sanitizedState = safeReturnTo(state, "/");
    let returnTo = sanitizedState.startsWith("/api") ? "/" : sanitizedState;

    // url is a query parameter that is only set by the Vercel auth flow
    // if it is set, and looks like a redirect to the device-auth flow,
    // we redirect to the device-auth flow.
    if (
      typeof url === "string" &&
      process.env.WORKOS_LOGIN_URL &&
      url.startsWith(process.env.WORKOS_LOGIN_URL)
    ) {
      returnTo = url;
    } else if (typeof path === "string" || typeof resource_id === "string") {
      const key = typeof path === "string" ? "vercelPath" : "projectId";
      const value = typeof path === "string" ? path : resource_id;
      returnTo = addQueryParam(returnTo, key, value as string);
    } else if (
      // @ts-expect-error VercelOAuth is a real authentication method
      authenticationMethod === "VercelOAuth" ||
      // @ts-expect-error VercelMarketplaceOAuth is a real authentication method
      authenticationMethod === "VercelMarketplaceOAuth"
    ) {
      returnTo = addQueryParam(returnTo, "vercelLogin", "true");
    }

    // Redirect the user to the homepage
    res.redirect(returnTo);
  } catch (error: any) {
    if (
      error.status === 403 &&
      error.rawData?.code === "email_verification_required"
    ) {
      res.redirect(
        `${process.env.WORKOS_LOGIN_URL}/email-verification?` +
          `email=${error.rawData.email}&` +
          `pending_authentication_token=${error.rawData?.pending_authentication_token}&` +
          `state=${state}&` +
          `redirect_uri=${state || "/"}`,
      );
      return;
    }
    captureException(error);
    // Show the friendly retry page. Without this the handler would fall through
    // without sending a response, hanging the function until Vercel times out.
    res.redirect(`/login-error?returnTo=${encodeURIComponent(state || "/")}`);
  }
}

function addQueryParam(url: string, key: string, value: string) {
  const symbol = url.includes("?") ? "&" : "?";
  return `${url}${symbol}${key}=${value}`;
}
