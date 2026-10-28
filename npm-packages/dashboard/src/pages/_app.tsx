import "../styles/global.css";

import type { AppProps } from "next/app";
import { useRouter } from "next/router";
import React from "react";
import { ErrorBoundary } from "@sentry/nextjs";
import { SWRConfig } from "swr";
import { swrConfig } from "hooks/swrConfig";
import { DashboardLayout } from "layouts/DashboardLayout";
import { DashboardHeader } from "components/header/DashboardHeader";
import { useInitialData } from "hooks/useServerSideData";
import { useRouterProgress } from "hooks/useRouterProgress";
import Head from "next/head";

import { Favicon } from "@common/elements/Favicon";
import { ThemeConsumer } from "@common/elements/ThemeConsumer";
import { ToastContainer } from "@common/elements/ToastContainer";
import { ThemeProvider } from "next-themes";
import { CurrentDeploymentDashboardLayout } from "layouts/DeploymentDashboardLayout";
import { DeploymentInfoProvider } from "providers/DeploymentInfoProvider";
import { MaybeDeploymentApiProvider } from "providers/MaybeDeploymentApiProvider";
import { PostHogProvider } from "providers/PostHogProvider";
import { SentryUserProvider } from "providers/SentryUserProvider";
import {
  LaunchDarklyConsumer,
  MaybeLaunchDarklyProvider,
} from "providers/LaunchDarklyProviders";
import { CommandPalette } from "elements/CommandPalette";
import { CreateProjectModal } from "hooks/useCreateProjectModal";
import { CreateTeamModal } from "components/header/CreateTeamModal";
import { JoinDirectorySyncedTeamModal } from "components/header/JoinDirectorySyncedTeamModal";
import { Fallback } from "pages/500";
import { UIProvider } from "@ui/UIContext";
import Link from "next/link";
import { RefreshSession } from "components/login/RefreshSession";
import { AuthProvider } from "providers/AuthProvider";
import { useSSOLoginRequired } from "api/api";
import { useDashboardVersion } from "hooks/useDashboardVersion";
import { SSOLoginRequired } from "components/login/SSOLoginRequired";

declare global {
  interface Window {
    gtag: any;
  }
}

if (typeof window !== "undefined") {
  // tells analytics that this is not frontend
  (window as any).isConsole = true;
}

const UNAUTHED_ROUTES = [
  "/404",
  "/login",
  "/login-error",
  "/signup",
  /^\/referral\/[A-Z0-9]+$/,
];

export default function App({ Component, pageProps }: AppProps) {
  useRouterProgress();

  return (
    <>
      <Head>
        <title>Convex Dashboard</title>
        <meta name="description" content="Manage your Convex apps" />
        <Favicon />
      </Head>
      <UIProvider Link={Link}>
        <AuthProvider>
          <PostHogProvider>
            <ThemeProvider attribute="class" disableTransitionOnChange>
              <ThemeConsumer />
              <MaybeLaunchDarklyProvider>
                <LaunchDarklyConsumer>
                  <AppInner Component={Component} pageProps={pageProps} />
                </LaunchDarklyConsumer>
              </MaybeLaunchDarklyProvider>
            </ThemeProvider>
          </PostHogProvider>
        </AuthProvider>
      </UIProvider>
    </>
  );
}

function AppInner({ Component, pageProps }: Omit<AppProps, "router">) {
  const router = useRouter();

  const [ssoLoginRequired] = useSSOLoginRequired();
  const pathWithoutQueryString = router.asPath.split("?")[0].split("#")[0];

  const inUnauthedRoute = UNAUTHED_ROUTES.some((r) =>
    typeof r === "string" ? r === router.pathname : r.test(router.pathname),
  );
  // To share state across page transitions we load deployment data in this
  // shared App component if the path looks like a deployment.
  const inDeployment = router.pathname.startsWith(
    "/t/[team]/[project]/[deploymentName]",
  );
  const isSsoLocked =
    !!ssoLoginRequired && ssoLoginRequired === router.query.team;

  const [initialData] = useInitialData();

  useDashboardVersion();

  return (
    <>
      <RefreshSession />
      <SentryUserProvider>
        <ErrorBoundary fallback={Fallback}>
          <SWRConfig
            value={{
              ...swrConfig(),
              fallback: { initialData },
            }}
          >
            <ToastContainer />

            {inUnauthedRoute ? (
              <Component {...pageProps} />
            ) : (
              <div className="flex h-screen flex-col">
                {/* On deployment pages the palette must live inside the
                    deployment providers so it can query the connected
                    deployment (e.g. to switch components); elsewhere it renders
                    here. Exactly one instance mounts, keyed off the branch
                    taken below. */}
                {(isSsoLocked || !inDeployment) && <CommandPalette />}
                <DashboardHeader />
                <CreateProjectModal />
                <CreateTeamModal />
                <JoinDirectorySyncedTeamModal />
                {isSsoLocked ? (
                  <SSOLoginRequired />
                ) : inDeployment ? (
                  <DeploymentInfoProvider>
                    <MaybeDeploymentApiProvider>
                      <CommandPalette />
                      <CurrentDeploymentDashboardLayout>
                        <ErrorBoundary
                          fallback={Fallback}
                          key={pathWithoutQueryString}
                        >
                          <Component {...pageProps} />
                        </ErrorBoundary>
                      </CurrentDeploymentDashboardLayout>
                    </MaybeDeploymentApiProvider>
                  </DeploymentInfoProvider>
                ) : (
                  <DashboardLayout>
                    <ErrorBoundary
                      fallback={Fallback}
                      key={pathWithoutQueryString}
                    >
                      <Component {...pageProps} />
                    </ErrorBoundary>
                  </DashboardLayout>
                )}
              </div>
            )}
          </SWRConfig>
        </ErrorBoundary>
      </SentryUserProvider>
    </>
  );
}
