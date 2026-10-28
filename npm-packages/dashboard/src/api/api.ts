import createClient, {
  RequestBodyOption,
  MaybeOptionalInit,
  HeadersOptions,
  FetchResponse,
} from "openapi-fetch";
import { PathsWithMethod } from "openapi-typescript-helpers";
import {
  createInfiniteHook,
  createMutateHook,
  createQueryHook,
  TypesForGetRequest,
} from "swr-openapi";
import isMatch from "lodash/isMatch";
import { fireGoogleAnalyticsEvent } from "elements/GoogleAnalytics";
import { toast } from "@common/lib/utils";
import type { paths as BigBrainPaths } from "generatedApi";
import type { paths as ManagementApiPaths } from "@convex-dev/platform/managementApi";
import { SWRConfiguration, SWRResponse, mutate as globalMutate } from "swr";
import { useAccessToken } from "hooks/useServerSideData";
import { useRouter } from "next/router";
import { useCallback, useEffect } from "react";
import { createGlobalState, usePrevious } from "react-use";
import { captureException } from "@sentry/nextjs";
import { getGoogleAnalyticsClientId, reportHttpError } from "../hooks/fetching";
import { forceCheckIsOnline } from "./onlineStatus";

export const client = createClient<BigBrainPaths>({
  baseUrl: `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/api/dashboard`,
});

// Management API client for stable platform APIs
export const managementApiClient = createClient<ManagementApiPaths>({
  baseUrl: `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/v1`,
});

// These are the error messages that we consider to be network errors, indicating that Big Brain may be offline.
const fetchErrorMessages = [
  "Failed to fetch", // Chromium
  "Load failed", // Safari
  "NetworkError when attempting to fetch resource.", // Firefox
];

const useQuery = createQueryHook(client, "big-brain");

export const useMutate = createMutateHook(client, "big-brain", isMatch);

// Management API hooks
const useManagementQuery = createQueryHook(
  managementApiClient,
  "management-api",
);

export const useManagementApiInfiniteQuery = createInfiniteHook(
  managementApiClient,
  "management-api",
);

export const useMutateManagementApi = createMutateHook(
  managementApiClient,
  "management-api",
  isMatch,
);

type Path<M extends "post" | "put" | "get" | "delete"> = PathsWithMethod<
  BigBrainPaths,
  M
>;
type ManagementPath<M extends "post" | "put" | "get" | "delete" | "patch"> =
  PathsWithMethod<ManagementApiPaths, M>;

export const useSSOLoginRequired = createGlobalState<string>();

// Helper to build API request headers
function useApiHeaders() {
  const googleAnalyticsId =
    typeof document !== "undefined" &&
    getGoogleAnalyticsClientId(document.cookie);
  const [accessToken] = useAccessToken();
  const authHeader = `Bearer ${accessToken}`;

  const headers: HeadersOptions = {
    Authorization: authHeader,
    "Convex-Client": "dashboard-0.0.0",
    "Google-Analytics-Client-Id": googleAnalyticsId,
  };

  return { headers, accessToken, authHeader, googleAnalyticsId };
}

// Helper to create SWR middleware that injects headers at fetch time
// This ensures headers are NOT part of the cache key
function createHeaderInjectionMiddleware(headers: HeadersOptions) {
  return (useSWRNext: any) => (key: any, fetcher: any, config: any) => {
    const wrappedFetcher = fetcher
      ? async (fetchKey: any) => {
          // Add headers to the request options before fetching
          // fetchKey structure: [prefix, path, requestOptions]
          if (Array.isArray(fetchKey) && fetchKey[2]) {
            const modifiedKey = [
              fetchKey[0],
              fetchKey[1],
              { ...fetchKey[2], headers },
            ];
            return fetcher(modifiedKey);
          }
          return fetcher(fetchKey);
        }
      : fetcher;
    return useSWRNext(key, wrappedFetcher, config);
  };
}

// Helper to validate auth header in mutations
function validateAuthHeader(authHeader: string) {
  if (!authHeader) {
    toast(
      "error",
      "An error occurred authenticating your request. Please try again.",
      "authHeader",
    );
    throw new Error("authHeader error");
  }
}

// Helper to handle mutation errors
function handleMutationError(
  method: string,
  path: string,
  error: any,
  toastOnError: boolean,
) {
  reportHttpError(
    method.toUpperCase(),
    path,
    error as unknown as { message: string; code: string },
  );
  if (toastOnError) {
    toast("error", error.message, error.message);
  }
}

// Helper to handle post-mutation success side effects
async function handleMutationSuccess(
  router: ReturnType<typeof useRouter>,
  redirectTo?: string,
  successToast?: string,
  googleAnalyticsEvent?: string,
  mutateKey?: string,
  cachePrefix?: "big-brain" | "management-api",
) {
  if (redirectTo) {
    await router.push(redirectTo);
  }
  if (successToast) toast("success", successToast);
  if (googleAnalyticsEvent) fireGoogleAnalyticsEvent(googleAnalyticsEvent);

  if (mutateKey && cachePrefix) {
    await globalMutate((key) => {
      if (!Array.isArray(key) || key.length < 3) return false;
      const [prefix, keyPath] = key;
      return prefix === cachePrefix && keyPath === mutateKey;
    });
  }
}

// Helper to handle query errors
function handleQueryError(res: any, path: string) {
  if (!("error" in res) || !res.error || typeof res.error !== "object") {
    return;
  }

  // `fetch` rejects with an Error (network failure, abort) rather than a Big
  // Brain error body. This must be checked before the body shape below: a
  // DOMException inherits `code` and `message`, so it would otherwise be
  // captured as "Server responded with 20 The operation was aborted.". It
  // must also be checked before `Object.keys`: Sentry's fetch instrumentation
  // backfills `error.stack` when the browser leaves it undefined (Safari, for
  // DOMExceptions), turning `stack` into an own enumerable property.
  if (res.error instanceof Error) {
    // Sentry's fetch instrumentation appends the request host to these
    // messages ("Failed to fetch (api.convex.dev)"), hence prefix matching.
    if (fetchErrorMessages.some((m) => res.error.message.startsWith(m))) {
      // Bypass the cache to learn whether we really went offline.
      void forceCheckIsOnline();
    }
    return;
  }

  if ("code" in res.error && "message" in res.error) {
    captureException(
      new Error(`Server responded with ${res.error.code} ${res.error.message}`),
      {
        fingerprint:
          res.error.code === "AccessTokenInvalid" ||
          res.error.code === "InvalidIdentity"
            ? [res.error.code]
            : [path, res.error.code as string],
      },
    );
    return;
  }

  if (Object.keys(res.error).length > 0) {
    captureException(
      new Error(`Server responded with error: ${JSON.stringify(res.error)}`),
      {
        // The body is interpolated into the message but kept out of the
        // fingerprint: it varies per occurrence, which would mint a fresh
        // Sentry issue (and a fresh alert) for every one of them.
        fingerprint: [path],
      },
    );
  }
}

export function useBBQuery<QueryPath extends Path<"get">>({
  path,
  pathParams,
  queryParams,
  swrOptions,
}: {
  path: QueryPath;
  pathParams: BigBrainPaths[QueryPath]["get"]["parameters"]["path"];
  queryParams?: BigBrainPaths[QueryPath]["get"]["parameters"]["query"];
  swrOptions?: Omit<SWRConfiguration, "onError">;
}): SWRResponse<
  TypesForGetRequest<BigBrainPaths, QueryPath>["Data"],
  TypesForGetRequest<BigBrainPaths, QueryPath>["Error"]
> {
  const router = useRouter();
  const [ssoLoginRequired, setSSOLoginRequired] = useSSOLoginRequired();
  const { headers, accessToken } = useApiHeaders();

  // Don't include headers in requestOptions - they'll be injected by middleware
  // This ensures headers are NOT part of the SWR cache key
  // @ts-expect-error TODO: Figure out how to type this.
  const requestOptions: MaybeOptionalInit<BigBrainPaths[QueryPath], "get"> = {
    params: {
      path: pathParams,
      query: queryParams,
    },
  };

  // If any path params are falsey, pause! Paused queries return undefined.
  const paused =
    !accessToken || (pathParams && Object.values(pathParams).some((p) => !p));
  const previousPaused = usePrevious(paused);
  const mutate = useMutate();

  useEffect(() => {
    if (previousPaused && !paused) {
      void mutate(
        // @ts-expect-error TODO: Figure out how to type this.
        [path, { params: { path: pathParams, query: queryParams } }],
        undefined,
      );
    }
  }, [paused, mutate, path, pathParams, queryParams, previousPaused]);

  const res = useQuery(path, requestOptions, {
    keepPreviousData: true,
    isPaused: () => paused,
    onError: (e) => {
      if ((e as any).code === "SSORequired" && !ssoLoginRequired) {
        setSSOLoginRequired(router.query.team as string);
      }
    },
    ...swrOptions,
    // Inject headers at fetch time via middleware, not in cache key
    use: [createHeaderInjectionMiddleware(headers), ...(swrOptions?.use || [])],
  });

  handleQueryError(res, path);
  return res;
}

// Makes a mutative API request, handling errors and toasts.
export function useBBMutation<
  T extends Path<Method>,
  M extends Path<"get">,
  Method extends "post" | "put" | "delete" = "post",
>({
  path,
  pathParams,
  successToast,
  toastOnError = true,
  googleAnalyticsEvent,
  redirectTo,
  includeCredentials = false,
  method = "post" as Method,
  ...rest
}: {
  path: T;
  method?: Method;
  pathParams: BigBrainPaths[T][Method] extends { parameters: {} }
    ? BigBrainPaths[T][Method]["parameters"]["path"]
    : undefined;
  successToast?: string;
  toastOnError?: boolean;
  redirectTo?: string;
  googleAnalyticsEvent?: string;
  includeCredentials?: boolean;
} & (
  | {
      mutateKey: M;
      mutatePathParams: BigBrainPaths[M]["get"]["parameters"]["path"];
    }
  | {}
)) {
  const router = useRouter();
  const { headers, authHeader } = useApiHeaders();

  type RequestBody = RequestBodyOption<BigBrainPaths[T][Method]>["body"];

  return useCallback(
    async (
      ...body: undefined extends RequestBody
        ? RequestBody extends undefined
          ? [] // the endpoint does not accept a request body
          : [] | [RequestBody] // the request body is optional
        : [RequestBody] // the request body is required
    ): Promise<
      FetchResponse<BigBrainPaths[T], any, "application/json">["data"]
    > => {
      validateAuthHeader(authHeader);

      const call =
        method === "put"
          ? client.PUT
          : method === "delete"
            ? client.DELETE
            : client.POST;

      const {
        error,
        data,
        response,
      }: FetchResponse<BigBrainPaths[T], any, "application/json"> =
        // @ts-expect-error TODO: Figure out how to type this.
        await call(path, {
          params: { path: pathParams },
          body: body[0],
          headers,
          credentials: includeCredentials ? "include" : "omit",
        });

      if (!response.ok && error) {
        handleMutationError(method, path, error, toastOnError);
        throw error;
      }

      await handleMutationSuccess(
        router,
        redirectTo,
        successToast,
        googleAnalyticsEvent,
        "mutateKey" in rest ? rest.mutateKey : undefined,
        "big-brain",
      );

      return data;
    },
    [
      authHeader,
      googleAnalyticsEvent,
      headers,
      includeCredentials,
      method,
      path,
      pathParams,
      redirectTo,
      rest,
      router,
      successToast,
      toastOnError,
    ],
  );
}

// Management API query hook - similar to useBBQuery but for stable platform APIs
export function useManagementApiQuery<QueryPath extends ManagementPath<"get">>({
  path,
  pathParams,
  queryParams,
  swrOptions,
}: {
  path: QueryPath;
  pathParams: ManagementApiPaths[QueryPath]["get"]["parameters"]["path"];
  queryParams?: ManagementApiPaths[QueryPath]["get"]["parameters"]["query"];
  swrOptions?: Omit<SWRConfiguration, "onError">;
}) {
  const router = useRouter();
  const [ssoLoginRequired, setSSOLoginRequired] = useSSOLoginRequired();
  const { headers, accessToken } = useApiHeaders();

  // Don't include headers in requestOptions - they'll be injected by middleware
  // This ensures headers are NOT part of the SWR cache key
  // @ts-expect-error TODO: Figure out how to type this.
  const requestOptions: MaybeOptionalInit<
    ManagementApiPaths[QueryPath],
    "get"
  > = {
    params: {
      path: pathParams,
      query: queryParams,
    },
  };

  // If any path params are falsey, pause! Paused queries return undefined.
  const paused =
    !accessToken || (pathParams && Object.values(pathParams).some((p) => !p));
  const previousPaused = usePrevious(paused);
  const mutate = useMutateManagementApi();

  useEffect(() => {
    if (previousPaused && !paused) {
      void mutate(
        // @ts-expect-error TODO: Figure out how to type this.
        [path, { params: { path: pathParams, query: queryParams } }],
        undefined,
      );
    }
  }, [paused, mutate, path, pathParams, queryParams, previousPaused]);

  const res = useManagementQuery(path, requestOptions, {
    keepPreviousData: true,
    isPaused: () => paused,
    onError: (e) => {
      if ((e as any).code === "SSORequired" && !ssoLoginRequired) {
        setSSOLoginRequired(router.query.team as string);
      }
    },
    ...swrOptions,
    // Inject headers at fetch time via middleware, not in cache key
    use: [createHeaderInjectionMiddleware(headers), ...(swrOptions?.use || [])],
  });

  handleQueryError(res, path);
  return res;
}

// Management API mutation hook - similar to useBBMutation but for stable platform APIs
export function useManagementApiMutation<
  T extends ManagementPath<Method>,
  M extends ManagementPath<"get">,
  Method extends "post" | "put" | "delete" | "patch" = "post",
>({
  path,
  pathParams,
  successToast,
  toastOnError = true,
  googleAnalyticsEvent,
  redirectTo,
  includeCredentials = false,
  method = "post" as Method,
  ...rest
}: {
  path: T;
  method?: Method;
  pathParams: ManagementApiPaths[T][Method] extends { parameters: {} }
    ? ManagementApiPaths[T][Method]["parameters"]["path"]
    : undefined;
  successToast?: string;
  toastOnError?: boolean;
  redirectTo?: string;
  googleAnalyticsEvent?: string;
  includeCredentials?: boolean;
} & (
  | {
      mutateKey: M;
      mutatePathParams: ManagementApiPaths[M]["get"]["parameters"]["path"];
    }
  | {}
)) {
  const router = useRouter();
  const { headers, authHeader } = useApiHeaders();

  type RequestBody = RequestBodyOption<ManagementApiPaths[T][Method]>["body"];

  return useCallback(
    async (
      ...body: undefined extends RequestBody
        ? RequestBody extends undefined
          ? [] // the endpoint does not accept a request body
          : [] | [RequestBody] // the request body is optional
        : [RequestBody] // the request body is required
    ): Promise<
      FetchResponse<ManagementApiPaths[T], any, "application/json">["data"]
    > => {
      validateAuthHeader(authHeader);

      const call =
        method === "put"
          ? managementApiClient.PUT
          : method === "delete"
            ? managementApiClient.DELETE
            : method === "patch"
              ? managementApiClient.PATCH
              : managementApiClient.POST;

      const {
        error,
        data,
        response,
      }: FetchResponse<ManagementApiPaths[T], any, "application/json"> =
        // @ts-expect-error TODO: Figure out how to type this.
        await call(path, {
          params: { path: pathParams },
          body: body[0],
          headers,
          credentials: includeCredentials ? "include" : "omit",
        });

      if (!response.ok && error) {
        handleMutationError(method, path, error, toastOnError);
        throw error;
      }

      await handleMutationSuccess(
        router,
        redirectTo,
        successToast,
        googleAnalyticsEvent,
        "mutateKey" in rest ? rest.mutateKey : undefined,
        "management-api",
      );

      return data;
    },
    [
      authHeader,
      googleAnalyticsEvent,
      headers,
      includeCredentials,
      method,
      path,
      pathParams,
      redirectTo,
      rest,
      router,
      successToast,
      toastOnError,
    ],
  );
}
