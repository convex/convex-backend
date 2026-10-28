import React, { useEffect } from "react";
import { useRouter } from "next/router";
import { useAsync } from "react-use";
import { basicLogger, LDClient, LDFlagSet } from "launchdarkly-js-client-sdk";
import {
  asyncWithLDProvider,
  withLDConsumer,
} from "launchdarkly-react-client-sdk";
import { LoadingLogo } from "@ui/Loading";
import { flagDefaultsKebabCase } from "hooks/useLaunchDarkly";
import { useGlobalLDContext, useLDContext } from "hooks/useLaunchDarklyContext";
import { useAccessToken } from "hooks/useServerSideData";

// LaunchDarkly cleaned up their API and not longer exposes this type
/**
 * The possible props the wrapped component can receive from the `LDConsumer` HOC.
 */
interface LDProps {
  /**
   * A map of feature flags from their keys to their values.
   * Keys are camelCased using `lodash.camelcase`.
   */
  flags?: LDFlagSet;

  /**
   * An instance of `LDClient` from the LaunchDarkly JS SDK (`launchdarkly-js-client-sdk`)
   *
   * @see https://docs.launchdarkly.com/sdk/client-side/javascript
   */
  ldClient?: LDClient;
}

export function LaunchDarklyProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const router = useRouter();

  const clientSideID = process.env.NEXT_PUBLIC_LAUNCHDARKLY_SDK_CLIENT_SIDE_ID;
  if (!clientSideID) {
    throw new Error("LaunchDarkly Client Side ID not set");
  }

  const [, setContext] = useGlobalLDContext();
  const localContext = useLDContext();
  useEffect(() => {
    if (!router.query.deploymentName && localContext) {
      setContext(localContext);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(localContext), router.query.deploymentName]);

  // For some reason, typing this properly still causes a type error ts(7022)
  const { value: LDProvider }: any = useAsync<
    () => ReturnType<typeof asyncWithLDProvider>
  >(
    async () =>
      LDProvider ||
      asyncWithLDProvider({
        clientSideID,
        options: {
          fetchGoals: false,
          logger: basicLogger({ level: "error" }),
        },
        flags: flagDefaultsKebabCase,
        // If in test mode, default the user to a test user with a stable key.
        context: process.env.NEXT_PUBLIC_TEST_MODE
          ? { key: "test", anonymous: true, kind: "user" }
          : // Otherwise, use an anonymous user with a stable key until we are logged in.
            // This prevents too many users from being created in LaunchDarkly.
            { key: "user", anonymous: true, kind: "user" },
      }),
    [],
  );

  return LDProvider ? (
    <LDProvider>{children}</LDProvider>
  ) : (
    <div className="flex h-screen w-full items-center justify-center">
      <LoadingLogo />
    </div>
  );
}

export function MaybeLaunchDarklyProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [accessToken] = useAccessToken();

  return accessToken ? (
    <LaunchDarklyProvider>{children}</LaunchDarklyProvider>
  ) : (
    <>{children}</>
  );
}

export const LaunchDarklyConsumer = withLDConsumer({ clientOnly: true })(
  function LaunchDarklyConsumer({
    ldClient,
    children,
  }: LDProps & { children: React.ReactElement }) {
    const [context] = useGlobalLDContext();
    useEffect(() => {
      // Don't identify users in test mode
      if (process.env.NEXT_PUBLIC_TEST_MODE) {
        return;
      }
      if (context) {
        void ldClient?.identify(context);
      }
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [JSON.stringify(context)]);
    return children;
  },
);
