import { NextApiRequest, NextApiResponse } from "next";
import {
  getSession,
  createSessionCookie,
  WorkOSUnavailableError,
} from "server/workos";

export default async function handler(
  req: NextApiRequest,
  res: NextApiResponse,
) {
  if (req.method !== "GET") {
    return res.status(405).json({ error: "Method not allowed" });
  }

  try {
    const session = await getSession(req);

    if (!session) {
      return res.status(401).json({ error: "Not authenticated" });
    }

    // If the session was refreshed, update the cookie
    if (session.sealedSession) {
      res.setHeader("Set-Cookie", createSessionCookie(session.sealedSession));
    }

    // Don't send sealedSession to the client
    const { sealedSession: _sealedSession, ...sessionData } = session;

    res.status(200).json(sessionData);
  } catch (error) {
    if (error instanceof WorkOSUnavailableError) {
      // 503 (not 401) so the client shows the "try again" page instead of
      // treating the user as logged out.
      console.error("WorkOS unavailable while getting session:", error);
      return res.status(503).json({
        error: "Authentication service unavailable",
        code: "AuthServiceUnavailable",
      });
    }
    console.error("Error getting session:", error);
    res.status(500).json({ error: "Internal server error" });
  }
}
