import { useRouter } from "next/router";
import { useHasOptedIn } from "api/optins";
import type { InferGetServerSidePropsType, NextPage } from "next";

import { getServerSideProps } from "lib/ssr";
import { JSX, useEffect } from "react";
import { useAccessToken, useInitialData } from "hooks/useServerSideData";
import { Button } from "@ui/Button";
import { Callout } from "@ui/Callout";
import { LoadingLogo } from "@ui/Loading";
import { Link } from "@ui/Link";
import { UIProvider } from "@ui/UIContext";
import { useWorkOS } from "hooks/useWorkOS";
import { logout } from "lib/logout";
import { User } from "@workos-inc/node";

interface UserProps {
  user: User;
}

// WorkOS version of withPageAuthRequired
const withPageAuthRequired = (
  Component: React.ComponentType<any>,
  options: any = {},
) =>
  function WithPageAuthRequired(props: any): JSX.Element {
    const {
      returnTo,
      onRedirecting = defaultOnRedirecting,
      onError = defaultOnError,
    } = options;
    const { user, error, isLoading } = useWorkOS();

    useEffect(() => {
      if ((user && !error) || isLoading) return;
      let returnToPath: string;

      if (!returnTo) {
        const currentLocation = window.location.toString();
        returnToPath =
          currentLocation.replace(new URL(currentLocation).origin, "") || "/";
      } else {
        returnToPath = returnTo;
      }

      window.location.assign(
        `/api/auth/login?returnTo=${encodeURIComponent(returnToPath)}`,
      );
    }, [user, error, isLoading, returnTo]);

    if (error) return onError(error);
    if (user) return <Component user={user} {...(props as any)} />;

    return onRedirecting();
  };

// Default loading component
const defaultOnRedirecting = () => (
  <div className="flex size-full items-center justify-center">
    <LoadingLogo />
  </div>
);

// Default error component
const defaultOnError = (error: Error) => (
  <div className="flex h-full flex-col items-center justify-center">
    <div className="mx-8 flex w-fit flex-col gap-4">
      <Callout variant="error">
        <div className="flex max-w-prose flex-col gap-2">
          Authentication error: {error.message}
        </div>
      </Callout>
    </div>
  </div>
);

export const withAuthenticatedPage = (Page: NextPage) =>
  withPageAuthRequired(withSWRFallback(Page));

function withSWRFallback(Page: NextPage) {
  return function WithAuthenticatedPage({
    accessToken,
    initialData,
    error,
  }: InferGetServerSidePropsType<typeof getServerSideProps> & UserProps) {
    const [globalAccessToken, setAccessToken] = useAccessToken();
    const [globalInitialData, setInitialData] = useInitialData();
    useEffect(() => {
      if (accessToken) {
        setAccessToken(accessToken);
      }
    }, [accessToken, setAccessToken]);
    useEffect(() => {
      if (!globalInitialData) {
        setInitialData(initialData);
      }
    }, [initialData, setInitialData, globalInitialData]);
    const { pathname } = useRouter();

    if (error) {
      const message =
        error?.message || "Failed to connect to the Convex dashboard.";
      let extra = null;

      if (error?.code === "FailedToConnect") {
        extra = (
          <p>
            {" "}
            Please try again or contact us at{" "}
            <Link
              href="mailto:support@convex.dev"
              passHref
              className="items-center"
            >
              support@convex.dev
            </Link>{" "}
            for support with this issue.
          </p>
        );
      }

      if (error?.code === "EmailAlreadyExists") {
        extra = (
          <p>
            If this is unexepected, please contact us at{" "}
            <Link
              href="mailto:support@convex.dev"
              passHref
              className="items-center"
            >
              support@convex.dev
            </Link>
          </p>
        );
      }

      return (
        <div className="h-full grow">
          <div className="flex h-full flex-col items-center justify-center">
            <div className="mx-8 flex w-fit flex-col gap-4">
              <Callout variant="error">
                <div className="flex max-w-prose flex-col gap-2">
                  {message} {extra}
                </div>
              </Callout>
              <UIProvider>
                <Button
                  onClick={() => logout()}
                  variant="neutral"
                  className="ml-auto w-fit"
                >
                  Log out
                </Button>
              </UIProvider>
            </div>
          </div>
        </div>
      );
    }
    return globalAccessToken ? (
      // When we're on the link_identity page, we don't want to render the OptinsRedirect
      // because it will make a request to big brain with an invalid access token.
      pathname === "/link_identity" ? (
        <Page />
      ) : (
        <OptinRedirect>
          <Page />
        </OptinRedirect>
      )
    ) : null;
  };
}

function OptinRedirect({ children }: { children: JSX.Element }) {
  const router = useRouter();
  const pathname = router.asPath !== "/" ? router.asPath : undefined;
  const { isLoading, hasOptedIn } = useHasOptedIn();
  if (!isLoading && !hasOptedIn && router.pathname !== "/accept") {
    void router.replace({
      pathname: "/accept",
      query: { from: pathname },
    });
  }

  return isLoading || (!hasOptedIn && router.pathname !== "/accept") ? (
    <div className="flex size-full items-center justify-center">
      <LoadingLogo />
    </div>
  ) : (
    children
  );
}
