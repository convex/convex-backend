import { NextApiRequest, NextApiResponse } from "next";
import {
  refreshSession,
  createSessionCookie,
  WorkOSUnavailableError,
} from "server/workos";

export default async function handler(
  req: NextApiRequest,
  res: NextApiResponse,
) {
  if (req.method !== "POST") {
    res.setHeader("Allow", "POST");
    res.status(405).end("Method Not Allowed");
    return;
  }

  try {
    const result = await refreshSession(req);

    if (!result) {
      return res.status(401).json({ error: "No valid session found" });
    }

    res.setHeader("Set-Cookie", createSessionCookie(result.sealedSession));
    res.status(200).json({ accessToken: result.accessToken });
  } catch (error: any) {
    if (error instanceof WorkOSUnavailableError) {
      console.error("WorkOS unavailable while refreshing session:", error);
      return res.status(503).json({
        error: "Authentication service unavailable",
        code: "AuthServiceUnavailable",
      });
    }
    console.error("Error refreshing session:", error);
    res.status(500).json({ error: error.message || "Internal server error" });
  }
}
