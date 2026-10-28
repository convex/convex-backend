import type { GetServerSideProps, GetServerSidePropsContext } from "next";
import {
  getAccessToken,
  withPageAuthRequired,
  WorkOSUnavailableError,
} from "server/workos";
import groupBy from "lodash/groupBy";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { TeamResponse, ProjectDetails, MemberPreferences } from "generatedApi";
import fetchRetryFactory from "fetch-retry";
import { getGoogleAnalyticsClientId } from "hooks/fetching";

const getProps: GetServerSideProps<{
  [key: string]: any;
}> = async ({ req, res, query, resolvedUrl }) => {
  const googleAnalyticsId = req.headers.cookie
    ? getGoogleAnalyticsClientId(req.headers.cookie)
    : "";

  const isFirstServerCall = req?.url?.indexOf("/_next/data/") === -1;
  const shouldRedirectToDeploymentPage = resolvedUrl.startsWith("/d/");
  const shouldRedirectToProjectPage = resolvedUrl.startsWith("/p/");
  const shouldRedirectToProjectPageFromDeployment =
    resolvedUrl.startsWith("/dp/");

  // If this is not the first time we're loading props, we can return early
  // and not re-fetch additional data.
  if (
    !isFirstServerCall &&
    !shouldRedirectToDeploymentPage &&
    !shouldRedirectToProjectPage &&
    !shouldRedirectToProjectPageFromDeployment
  ) {
    return { props: {} };
  }

  let token: { accessToken: string } | null;
  try {
    token = await getAccessToken(req);
  } catch (error) {
    if (error instanceof WorkOSUnavailableError) {
      // WorkOS is unreachable. Send the user to the retry pagerather
      res.writeHead(307, {
        Location: `/login-error?returnTo=${encodeURIComponent(req.url || "/")}`,
      });
      res.end();
      return { props: {} };
    }
    console.error("Couldn't fetch auth token", error);
    // If we can't get the token, we should try to login again
    res.writeHead(307, {
      Location: `/api/auth/login?returnTo=${req.url}`,
    });
    res.end();
    return { props: {} };
  }

  if (!token) {
    return { props: {} };
  }

  if (process.env.DISABLE_BIG_BRAIN_SSR) {
    return {
      props: { accessToken: token.accessToken },
    };
  }

  // Create abort controller with 30 second timeout
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 30 * 1000);

  try {
    const headers: Record<string, string> = {
      authorization: `Bearer ${token.accessToken}`,
      "Google-Analytics-Client-Id": googleAnalyticsId,
    };

    const resp = await retryingFetch(
      `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/api/dashboard/member_data`,
      {
        headers,
        signal: controller.signal,
      },
    );
    if (!resp.ok) {
      try {
        const error: { message: string; code: string } = await resp.json();
        if (
          error.code === "InvalidIdentity" &&
          !resolvedUrl.startsWith("/link_identity")
        ) {
          const hint = error.message.split("(hint:")[1]?.split(")")[0];
          res.writeHead(307, {
            Location: `/link_identity?returnTo=${req.url}&hint=${hint}`,
          });
          res.end();
          return { props: {} };
        }
        if (resp.status === 400) {
          return {
            props: {
              error,
            },
          };
        }

        console.error("Couldn't fetch member data", error);
        return {
          props: {
            accessToken: token.accessToken,
            error,
          },
        };
      } catch (e) {
        console.error("Couldn't fetch member data", e);
        return {
          props: {
            accessToken: token.accessToken,
            error: {
              message: "Failed to connect to Convex dashboard",
              code: "FailedToConnect",
            },
          },
        };
      }
    }

    // Note that the response only includes data for the first MAX_PROJECTS_PER_PAGE projects
    const {
      teams,
      projects,
      deployments,
      optInsToAccept,
      preferences,
    }: {
      teams: TeamResponse[];
      projects: ProjectDetails[];
      deployments: PlatformDeploymentResponse[];
      optInsToAccept?: {
        optIn: string;
        message: string;
      }[];
      preferences?: MemberPreferences;
    } = await resp.json();
    const { team, project, deploymentName } = query;
    if (team && !teams.find((t: TeamResponse) => t.slug === team.toString())) {
      // You're looking for a page that doesn't exist!
      return pageNotFound(res);
    }

    if (shouldRedirectToProjectPage && project !== undefined) {
      return await redirectToProjectPage(
        resolvedUrl,
        res,
        project as string,
        token.accessToken,
      );
    }

    if (shouldRedirectToDeploymentPage && deploymentName !== undefined) {
      return await redirectToDeploymentPage(
        resolvedUrl,
        res,
        deploymentName as string,
        token.accessToken,
      );
    }

    if (
      shouldRedirectToProjectPageFromDeployment &&
      deploymentName !== undefined
    ) {
      return await redirectToProjectPageFromDeploymentName(
        resolvedUrl,
        res,
        deploymentName as string,
        token.accessToken,
      );
    }

    const projectsByTeam = groupBy(projects, (p: ProjectDetails) => p.teamId);

    const initialProjectsByTeam = Object.fromEntries(
      teams.map(({ id: teamId }) => [
        `/teams/${teamId}/projects`,
        {
          items: projectsByTeam[teamId] ?? [],
          pagination: { hasMore: false },
        },
      ]),
    );

    const initialIndividualProjects = Object.fromEntries(
      projects.map(({ id: projectId }) => [
        `/projects/${projectId}`,
        projects.find((p: ProjectDetails) => p.id === projectId),
      ]),
    );

    const deploymentsByProject = groupBy(
      deployments,
      (d: PlatformDeploymentResponse) => d.projectId,
    );
    const initialDeployments = Object.fromEntries(
      projects.map(({ id: projectId }) => [
        `/projects/${projectId}/list_deployments`,
        deploymentsByProject[projectId] ?? [],
      ]),
    );

    const initialData: Record<string, object> = {
      "/teams": teams,
      ...initialProjectsByTeam,
      ...initialIndividualProjects,
      ...initialDeployments,
      "/preferences": { preferences: preferences ?? {} },
    };

    if (optInsToAccept !== undefined) {
      initialData["/optins"] = { optInsToAccept };
    }

    return {
      props: {
        accessToken: token.accessToken,
        initialData,
      },
      redirect:
        optInsToAccept &&
        optInsToAccept.length > 0 &&
        !resolvedUrl.startsWith("/accept")
          ? {
              destination: `/accept${resolvedUrl ? `?from=${encodeURIComponent(resolvedUrl)}` : ""}`,
              permanent: false,
            }
          : undefined,
    };
  } catch (e: unknown) {
    const isFetchError = e instanceof FetchError;
    const isTimeout = isFetchError && e.isTimeout;

    return {
      props: {
        accessToken: token.accessToken,
        error: {
          message: isTimeout
            ? "Request timed out connecting to Convex dashboard"
            : isFetchError
              ? e.message
              : "Failed to connect to Convex dashboard",
          code: "FailedToConnect",
        },
      },
    };
  } finally {
    clearTimeout(timeoutId);
  }
};

export const getServerSideProps = withPageAuthRequired({
  getServerSideProps: getProps,
});

async function redirectToProjectPage(
  resolvedUrl: string,
  res: GetServerSidePropsContext["res"],
  projectSlug: string,
  accessToken: string,
) {
  try {
    // Fetch all teams to find which team owns this project
    const teamsResp = await retryingFetch(
      `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/api/dashboard/teams`,
      {
        headers: {
          authorization: `Bearer ${accessToken}`,
        },
      },
    );

    if (!teamsResp.ok) {
      return pageNotFound(res);
    }

    const teams: TeamResponse[] = await teamsResp.json();

    // Try each team to find the project
    let project: ProjectDetails | undefined;
    let owningTeam: TeamResponse | undefined;

    for (const team of teams) {
      try {
        const projectResp = await retryingFetch(
          `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/api/dashboard/teams/${team.id}/projects/${projectSlug}`,
          {
            headers: {
              authorization: `Bearer ${accessToken}`,
            },
          },
        );

        if (projectResp.ok) {
          project = await projectResp.json();
          owningTeam = team;
          break;
        }
      } catch {
        // Continue to next team
        continue;
      }
    }

    if (owningTeam === undefined || project === undefined) {
      return pageNotFound(res);
    }

    const remainingPath = resolvedUrl.slice(`/p/${projectSlug}`.length);
    return {
      redirect: {
        permanent: false,
        destination: `/t/${owningTeam.slug}/${project.slug}${remainingPath}`,
      },
    };
  } catch {
    return pageNotFound(res);
  }
}

async function redirectToDeploymentPage(
  resolvedUrl: string,
  res: GetServerSidePropsContext["res"],
  deploymentName: string,
  accessToken: string,
) {
  try {
    // Fetch team and project info for the deployment in a single call
    const teamAndProjectResp = await retryingFetch(
      `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/api/deployment/${deploymentName}/team_and_project`,
      {
        headers: {
          authorization: `Bearer ${accessToken}`,
        },
      },
    );

    if (!teamAndProjectResp.ok) {
      return pageNotFound(res);
    }

    const { team, project }: { team: string; project: string } =
      await teamAndProjectResp.json();

    const remainingPath = resolvedUrl.slice(`/d/${deploymentName}`.length);
    return {
      redirect: {
        permanent: false,
        destination: `/t/${team}/${project}/${deploymentName}${remainingPath}`,
      },
    };
  } catch (error) {
    console.error("Error redirecting to deployment page", error);
    return pageNotFound(res);
  }
}

async function redirectToProjectPageFromDeploymentName(
  resolvedUrl: string,
  res: GetServerSidePropsContext["res"],
  deploymentName: string,
  accessToken: string,
) {
  try {
    // Fetch team and project info for the deployment in a single call
    const teamAndProjectResp = await retryingFetch(
      `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/api/deployment/${deploymentName}/team_and_project`,
      {
        headers: {
          authorization: `Bearer ${accessToken}`,
        },
      },
    );

    if (!teamAndProjectResp.ok) {
      return pageNotFound(res);
    }

    const { team, project }: { team: string; project: string } =
      await teamAndProjectResp.json();

    const remainingPath = resolvedUrl.slice(`/dp/${deploymentName}`.length);
    return {
      redirect: {
        permanent: false,
        destination: `/t/${team}/${project}${remainingPath}`,
      },
    };
  } catch (error) {
    console.error("Error redirecting to project page from deployment", error);
    return pageNotFound(res);
  }
}

function pageNotFound(res: GetServerSidePropsContext["res"]) {
  res.writeHead(307, { Location: "/404" });
  res.end();
  return { props: {} };
}

// Custom error type for fetch failures
class FetchError extends Error {
  constructor(
    message: string,
    public readonly isTimeout: boolean = false,
    public readonly originalError?: unknown,
  ) {
    super(message);
    this.name = "FetchError";
  }
}

const baseFetch = fetchRetryFactory(fetch, {
  retries: 2,
  retryDelay: (attempt: number, _error: any, _response: any) => {
    // immediate, 1s delay, 2s delay, 4s delay, etc.
    const delay = attempt === 0 ? 1 : 2 ** (attempt - 1) * 1000;
    const randomSum = delay * 0.2 * Math.random();
    return delay + randomSum;
  },
  retryOn(_attempt: number, error: Error | null, response: Response | null) {
    // Don't retry on client errors (4xx)
    if (response && response.status >= 400 && response.status < 500) {
      return false;
    }
    // Retry on network errors, but not on aborted requests
    if (error) {
      // Don't retry if the request was aborted (timeout or manual abort)
      if (error.name === "AbortError") {
        return false;
      }
      // TODO filter out all SSL errors
      // https://github.com/nodejs/node/blob/8a41d9b636be86350cd32847c3f89d327c4f6ff7/src/crypto/crypto_common.cc#L218-L245
      return true;
    }
    // Retry on 5xx server errors
    if (response && response.status >= 500) {
      return true;
    }
    return false;
  },
});

// Wrapper that ensures all errors are typed as FetchError
export async function retryingFetch(
  url: string,
  options?: RequestInit,
): Promise<Response> {
  try {
    return await baseFetch(url, options);
  } catch (error) {
    // Check if it's an abort error (timeout)
    const isTimeout = error instanceof Error && error.name === "AbortError";

    // Get error message safely
    const message =
      error instanceof Error ? error.message : "Unknown error during fetch";

    throw new FetchError(message, isTimeout, error);
  }
}
