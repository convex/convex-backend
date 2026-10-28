import React, {
  createContext,
  ReactNode,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { AuthTokenFetcher } from "../browser/sync/client.js";
import { ConvexProvider } from "./client.js";

// Until we can import from our own entry points (requires TypeScript 4.7),
// just describe the interface enough to help users pass the right type.
type IConvexReactClient = {
  setAuth(
    fetchToken: AuthTokenFetcher,
    onChange: (isAuthenticated: boolean) => void,
    onRefreshChange?: (isRefreshing: boolean) => void,
  ): void;
  clearAuth(): void;
};

/**
 * Type representing the state of an auth integration with Convex.
 *
 * - `isLoading`: the client is still resolving the initial auth state and
 *   waiting for the server to confirm the current token.
 * - `isAuthenticated`: the server has confirmed the current token.
 * - `isRefreshing`: the server rejected a previously-confirmed token and the
 *   socket is paused while a replacement is fetched. Only ever `true` when
 *   `isAuthenticated` is also `true`. Routine background token rotation does
 *   not trigger this state.
 *
 * @public
 */
export type ConvexAuthState = {
  isLoading: boolean;
  isAuthenticated: boolean;
  isRefreshing: boolean;
};

const ConvexAuthContext = createContext<ConvexAuthState>(undefined as any);

/**
 * Get the {@link ConvexAuthState} within a React component.
 *
 * This relies on a Convex auth integration provider being above in the React
 * component tree. See {@link ConvexAuthState} for the meaning of each field.
 *
 * @returns The current {@link ConvexAuthState}.
 *
 * @public
 */
export function useConvexAuth(): {
  isLoading: boolean;
  isAuthenticated: boolean;
  isRefreshing: boolean;
} {
  const authContext = useContext(ConvexAuthContext);
  if (authContext === undefined) {
    throw new Error(
      "Could not find `ConvexProviderWithAuth` (or `ConvexProviderWithClerk` " +
        "or `ConvexProviderWithAuth0`) " +
        "as an ancestor component. This component may be missing, or you " +
        "might have two instances of the `convex/react` module loaded in your " +
        "project.",
    );
  }
  return authContext;
}

/**
 * A replacement for {@link ConvexProvider} which additionally provides
 * {@link ConvexAuthState} to descendants of this component.
 *
 * Use this to integrate any auth provider with Convex. The `useAuth` prop
 * should be a React hook that returns the provider's authentication state
 * and a function to fetch a JWT access token.
 *
 * If the `useAuth` prop function updates causing a rerender then auth state
 * will transition to loading and the `fetchAccessToken()` function called again.
 *
 * See [Custom Auth Integration](https://docs.convex.dev/auth/advanced/custom-auth) for more information.
 *
 * @public
 */
export function ConvexProviderWithAuth({
  children,
  client,
  useAuth,
}: {
  children?: ReactNode;
  client: IConvexReactClient;
  useAuth: () => {
    isLoading: boolean;
    isAuthenticated: boolean;
    fetchAccessToken: (args: {
      forceRefreshToken: boolean;
    }) => Promise<string | null>;
  };
}) {
  const {
    isLoading: authProviderLoading,
    isAuthenticated: authProviderAuthenticated,
    fetchAccessToken,
  } = useAuth();
  const [isConvexAuthenticated, setIsConvexAuthenticated] = useState<
    boolean | null
  >(null);
  const [isRefreshing, setIsRefreshing] = useState<boolean>(false);

  // If the useAuth went back to the authProviderLoading state (which is unusual but possible)
  // reset the Convex auth state to null so that we can correctly
  // transition the state from "loading" to "authenticated"
  // without going through "unauthenticated".
  if (authProviderLoading && isConvexAuthenticated !== null) {
    setIsConvexAuthenticated(null);
    setIsRefreshing(false);
  }

  // If the useAuth goes to not authenticated then isConvexAuthenticated should reflect that.
  if (
    !authProviderLoading &&
    !authProviderAuthenticated &&
    isConvexAuthenticated !== false
  ) {
    setIsConvexAuthenticated(false);
    setIsRefreshing(false);
  }

  const isAuthenticated =
    authProviderAuthenticated && (isConvexAuthenticated ?? false);
  const isLoading = isConvexAuthenticated === null;
  const isRefreshingWhileAuthenticated = isRefreshing && isAuthenticated;

  // Stabilize the context value's identity so `useConvexAuth()` consumers
  // only re-render when the derived auth state actually changes.
  const authState = useMemo<ConvexAuthState>(
    () => ({
      isLoading,
      isAuthenticated,
      isRefreshing: isRefreshingWhileAuthenticated,
    }),
    [isLoading, isAuthenticated, isRefreshingWhileAuthenticated],
  );

  return (
    <ConvexAuthContext.Provider value={authState}>
      <ConvexAuthStateFirstEffect
        authProviderAuthenticated={authProviderAuthenticated}
        fetchAccessToken={fetchAccessToken}
        authProviderLoading={authProviderLoading}
        client={client}
        setIsConvexAuthenticated={setIsConvexAuthenticated}
        setIsRefreshing={setIsRefreshing}
      />
      <ConvexProvider client={client as any}>{children}</ConvexProvider>
      <ConvexAuthStateLastEffect
        authProviderAuthenticated={authProviderAuthenticated}
        fetchAccessToken={fetchAccessToken}
        authProviderLoading={authProviderLoading}
        client={client}
        setIsConvexAuthenticated={setIsConvexAuthenticated}
        setIsRefreshing={setIsRefreshing}
      />
    </ConvexAuthContext.Provider>
  );
}

// First child ensures we `setAuth` before
// other child components subscribe to queries via `useEffect`.
function ConvexAuthStateFirstEffect({
  authProviderAuthenticated,
  fetchAccessToken,
  authProviderLoading,
  client,
  setIsConvexAuthenticated,
  setIsRefreshing,
}: {
  authProviderAuthenticated: boolean;
  fetchAccessToken: (args: {
    forceRefreshToken: boolean;
  }) => Promise<string | null>;
  authProviderLoading: boolean;
  client: IConvexReactClient;
  setIsConvexAuthenticated: React.Dispatch<
    React.SetStateAction<boolean | null>
  >;
  setIsRefreshing: React.Dispatch<React.SetStateAction<boolean>>;
}) {
  useEffect(() => {
    let isThisEffectRelevant = true;
    if (authProviderAuthenticated) {
      client.setAuth(
        fetchAccessToken,
        (backendReportsIsAuthenticated) => {
          if (isThisEffectRelevant) {
            setIsConvexAuthenticated(() => backendReportsIsAuthenticated);
          }
        },
        (isRefreshing) => {
          if (isThisEffectRelevant) {
            setIsRefreshing(isRefreshing);
          }
        },
      );
      return () => {
        isThisEffectRelevant = false;

        // If unmounting or something changed before we finished fetching the token
        // we shouldn't transition to a loaded state.
        setIsConvexAuthenticated((isConvexAuthenticated: boolean | null) =>
          isConvexAuthenticated ? false : null,
        );
        setIsRefreshing(false);
      };
    }
  }, [
    authProviderAuthenticated,
    fetchAccessToken,
    authProviderLoading,
    client,
    setIsConvexAuthenticated,
    setIsRefreshing,
  ]);
  return null;
}

// Last child ensures we `clearAuth` last,
// so that queries from unmounted sibling components
// unsubscribe first and don't rerun without auth on the server
function ConvexAuthStateLastEffect({
  authProviderAuthenticated,
  fetchAccessToken,
  authProviderLoading,
  client,
  setIsConvexAuthenticated,
  setIsRefreshing,
}: {
  authProviderAuthenticated: boolean;
  fetchAccessToken: (args: {
    forceRefreshToken: boolean;
  }) => Promise<string | null>;
  authProviderLoading: boolean;
  client: IConvexReactClient;
  setIsConvexAuthenticated: React.Dispatch<
    React.SetStateAction<boolean | null>
  >;
  setIsRefreshing: React.Dispatch<React.SetStateAction<boolean>>;
}) {
  useEffect(() => {
    // If rendered with authProviderAuthenticated=true then clear that auth on in cleanup.
    if (authProviderAuthenticated) {
      return () => {
        client.clearAuth();
        // Set state back to loading in case this is a transition from one
        // fetchToken function to another which signals a new auth context,
        // e.g. a new orgId from Clerk. Auth context changes like this
        // return isAuthenticated: true from useAuth() but if
        // useAuth reports isAuthenticated: false on the next render
        // then this null value will be overridden to false.
        setIsConvexAuthenticated(() => null);
        setIsRefreshing(false);
      };
    }
  }, [
    authProviderAuthenticated,
    fetchAccessToken,
    authProviderLoading,
    client,
    setIsConvexAuthenticated,
    setIsRefreshing,
  ]);
  return null;
}
