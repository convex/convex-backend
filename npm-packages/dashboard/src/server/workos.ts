import { NextApiRequest, GetServerSidePropsContext } from "next";
import { WorkOS } from "@workos-inc/node";

let instance: WorkOS | undefined;

export const WORKOS_REQUEST_TIMEOUT_MS = 10_000;

/**
 * Thrown when a WorkOS request exceeds {@link WORKOS_REQUEST_TIMEOUT_MS}.
 * Distinct from a `null` session (genuinely unauthenticated) so callers can
 * send the user to the "try again" page instead of a login loop that would hit
 * the same unavailable WorkOS.
 */
export class WorkOSUnavailableError extends Error {
  constructor(operation: string) {
    super(`WorkOS request timed out (${operation})`);
    this.name = "WorkOSUnavailableError";
  }
}

export async function withWorkOSTimeout<T>(
  operation: string,
  promise: Promise<T>,
): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        timeoutId = setTimeout(
          () => reject(new WorkOSUnavailableError(operation)),
          WORKOS_REQUEST_TIMEOUT_MS,
        );
      }),
    ]);
  } finally {
    if (timeoutId !== undefined) {
      clearTimeout(timeoutId);
    }
  }
}

export function getWorkOS() {
  if (instance !== undefined) {
    return instance;
  }

  instance = new WorkOS(process.env.WORKOS_API_SECRET, {
    clientId: process.env.WORKOS_CLIENT_ID,
  });
  return instance;
}

export interface WorkOSSession {
  user: {
    id: string;
    email: string;
    emailVerified: boolean;
    firstName?: string;
    lastName?: string;
    profilePictureUrl?: string;
  };
  accessToken?: string;
  impersonator?: {
    email: string;
    reason?: string;
  };
  sealedSession?: string;
}

export function loadSealedSessionFromRequest(
  req: NextApiRequest | GetServerSidePropsContext["req"],
) {
  const cookieHeader = req.headers.cookie;
  if (!cookieHeader) {
    return null;
  }

  const sessionCookie = cookieHeader
    .split(";")
    .find((cookie) => cookie.trim().startsWith("wos-session="))
    ?.split("=")[1];

  if (!sessionCookie) {
    return null;
  }

  const workosInstance = getWorkOS();
  return workosInstance.userManagement.loadSealedSession({
    sessionData: sessionCookie,
    cookiePassword: process.env.WORKOS_COOKIE_PASSWORD || "",
  });
}

export function createSessionCookie(sealedSession: string): string {
  const secure = process.env.NODE_ENV === "production" ? " Secure;" : "";
  return `wos-session=${sealedSession}; Path=/; HttpOnly;${secure} SameSite=Lax; Max-Age=${60 * 60 * 24 * 14}`;
}

export function deleteSessionCookie(): string {
  const secure = process.env.NODE_ENV === "production" ? " Secure;" : "";
  return `wos-session=deleted; Max-Age=-1; Path=/; HttpOnly;${secure} SameSite=Lax`;
}

async function refreshCookieSession(
  session: ReturnType<
    ReturnType<typeof getWorkOS>["userManagement"]["loadSealedSession"]
  >,
) {
  const refreshResult = await withWorkOSTimeout("refresh", session.refresh());

  if (
    !refreshResult.authenticated ||
    !refreshResult.sealedSession ||
    !refreshResult.session?.accessToken
  ) {
    return null;
  }

  return {
    accessToken: refreshResult.session.accessToken,
    sealedSession: refreshResult.sealedSession,
    user: refreshResult.session.user,
  };
}

export async function getSession(
  req: NextApiRequest | GetServerSidePropsContext["req"],
): Promise<WorkOSSession | null> {
  try {
    const session = loadSealedSessionFromRequest(req);

    if (!session) {
      return null;
    }

    const sess = await withWorkOSTimeout(
      "authenticate",
      session.authenticate(),
    );
    if (!sess.authenticated) {
      // Token expired, try to refresh
      const refreshed = await refreshCookieSession(session);
      if (!refreshed) {
        return null;
      }

      return {
        user: {
          id: refreshed.user.id,
          email: refreshed.user.email,
          emailVerified: refreshed.user.emailVerified,
          firstName: refreshed.user.firstName || "",
          lastName: refreshed.user.lastName || "",
          profilePictureUrl: refreshed.user.profilePictureUrl || "",
        },
        accessToken: refreshed.accessToken,
        sealedSession: refreshed.sealedSession,
      };
    }
    const { user, accessToken } = sess;
    return {
      user: {
        id: user.id,
        email: user.email,
        emailVerified: user.emailVerified,
        firstName: user.firstName || "",
        lastName: user.lastName || "",
        profilePictureUrl: user.profilePictureUrl || "",
      },
      accessToken,
    };
  } catch (error) {
    // Surface an unreachable WorkOS to the caller so it can show the
    // "try again" page; treat every other error as unauthenticated.
    if (error instanceof WorkOSUnavailableError) {
      throw error;
    }
    console.error("Error loading WorkOS session:", error);
    return null;
  }
}

export async function getAccessToken(
  req: NextApiRequest | GetServerSidePropsContext["req"],
): Promise<{ accessToken: string } | null> {
  const session = await getSession(req);
  if (!session || !session.accessToken) {
    return null;
  }

  return { accessToken: session.accessToken };
}

export async function refreshSession(
  req: NextApiRequest,
): Promise<{ accessToken: string; sealedSession: string } | null> {
  try {
    const session = loadSealedSessionFromRequest(req);

    if (!session) {
      return null;
    }

    const refreshed = await refreshCookieSession(session);
    if (!refreshed) {
      return null;
    }

    return {
      accessToken: refreshed.accessToken,
      sealedSession: refreshed.sealedSession,
    };
  } catch (error) {
    if (error instanceof WorkOSUnavailableError) {
      throw error;
    }
    console.error("Error refreshing WorkOS session:", error);
    return null;
  }
}

export interface WithPageAuthRequiredOptions {
  getServerSideProps?: (context: GetServerSidePropsContext) => Promise<any>;
  returnTo?: string;
}

export function withPageAuthRequired(
  options: WithPageAuthRequiredOptions = {},
) {
  return async (context: GetServerSidePropsContext) => {
    let session: WorkOSSession | null;
    try {
      session = await getSession(context.req);
    } catch (error) {
      if (error instanceof WorkOSUnavailableError) {
        return {
          redirect: {
            destination: `/login-error?returnTo=${encodeURIComponent(
              context.resolvedUrl,
            )}`,
            permanent: false,
          },
        };
      }
      throw error;
    }

    // If there's a custom getServerSideProps, call it
    if (options.getServerSideProps) {
      const result = await options.getServerSideProps(context);

      // Merge session info with the result
      if (result.props) {
        result.props.session = session;
      }

      return result;
    }

    return {
      props: {
        session,
      },
    };
  };
}
