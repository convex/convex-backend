import React, {
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useDebounce, usePrevious } from "react-use";
import { useRouter, type NextRouter } from "next/router";
import type { ParsedUrlQuery } from "querystring";
import isEqual from "lodash/isEqual";
import { dismissToast, toast } from "@common/lib/utils";
import { LogList } from "@common/features/logs/components/LogList";
import { LogToolbar } from "@common/features/logs/components/LogToolbar";
import { SearchLogsInput } from "@common/features/logs/components/SearchLogsInput";
import { filterLogs } from "@common/features/logs/lib/filterLogs";
import { NENT_APP_PLACEHOLDER, Nent } from "@common/lib/useNents";
import {
  itemIdentifier,
  useModuleFunctions,
} from "@common/lib/functions/FunctionsProvider";
import {
  functionIdentifierFromValue,
  functionIdentifierValue,
} from "@common/lib/functions/generateFileTree";
import { MAX_LOGS, UdfLog, useLogs } from "@common/lib/useLogs";
import { useDeploymentAuditLogs } from "@common/lib/useDeploymentAuditLog";
import { Button } from "@ui/Button";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { MultiSelectValue } from "@ui/MultiSelectCombobox";

function logsQuerySignature(query: ParsedUrlQuery) {
  return JSON.stringify({
    filter: query.filter,
    components: query.components,
    functions: query.functions,
    logTypes: query.logTypes,
  });
}

function parseLogSelection(
  value: string | string[] | undefined,
  isValid: (option: string) => boolean = () => true,
): MultiSelectValue | undefined {
  if (value === "all") return "all";
  if (typeof value !== "string") return undefined;
  try {
    const selection: unknown = JSON.parse(value);
    return Array.isArray(selection) &&
      selection.every((option) => typeof option === "string" && isValid(option))
      ? selection
      : undefined;
  } catch {
    return undefined;
  }
}

function isValidFunctionSelection(value: string) {
  const identifier = functionIdentifierFromValue(value);
  return (
    typeof identifier.identifier === "string" &&
    (identifier.componentPath === undefined ||
      typeof identifier.componentPath === "string") &&
    (identifier.componentId === undefined ||
      typeof identifier.componentId === "string")
  );
}

function serializeLogSelection(selection: MultiSelectValue) {
  return selection === "all" ? undefined : JSON.stringify(selection);
}

function logsUrl(router: NextRouter, query: ParsedUrlQuery) {
  const cleanQuery = { ...query };
  for (const key of ["components", "functions", "logTypes"] as const) {
    if (cleanQuery[key] === undefined || cleanQuery[key] === "all") {
      delete cleanQuery[key];
    }
  }
  if (cleanQuery.filter === undefined || cleanQuery.filter === "") {
    delete cleanQuery.filter;
  }
  // Next.js also exposes dynamic path segments through router.query.
  for (const match of router.pathname.matchAll(
    /\[{1,2}(?:\.\.\.)?([^\]]+)\]{1,2}/g,
  )) {
    delete cleanQuery[match[1]];
  }
  const [pathAndSearch, hash] = router.asPath.split("#");
  return {
    pathname: pathAndSearch.split("?")[0],
    query: cleanQuery,
    ...(hash ? { hash: `#${hash}` } : {}),
  };
}

export function Logs({
  nents: allNents,
  selectedNent,
}: {
  nents: Nent[];
  selectedNent: Nent | null;
}) {
  const router = useRouter();
  const { query } = router;
  const querySignature = logsQuerySignature(query);
  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const deployment = useCurrentDeployment();
  const deploymentPrefix = deployment?.name;

  const nents = allNents.filter((nent) => nent.name !== null);
  const logsConnectivityCallbacks = useRef({
    onReconnected: () => {
      dismissToast("logStreamError");
      toast("info", "Reconnected to log stream.", "logStreamReconnected");
    },
    onDisconnected: () => {
      dismissToast("logStreamReconnected");
      toast(
        "error",
        "Disconnected from log stream. Will attempt to reconnect automatically.",
        "logStreamError",
        false,
      );
    },
    onPermissionDenied: (message: string) => {
      toast("error", message, "logStreamPermissionDenied", false);
    },
  });

  // Manage state for filter text.
  const [filter, setFilter] = useGlobalLocalStorage(
    `logs/${deploymentPrefix}/filter`,
    "",
  );

  // Manage state for current log levels.
  const [levels, setLevels] = useGlobalLocalStorage<MultiSelectValue>(
    `logs/${deploymentPrefix}/levels`,
    "all",
  );

  const defaultSelectedNent: MultiSelectValue = "all";

  const [selectedNents, setSelectedNents] =
    useGlobalLocalStorage<MultiSelectValue>(
      `logs/${deploymentPrefix}/selectedNents`,
      defaultSelectedNent,
    );

  const moduleFunctions = useModuleFunctions();
  const functions = useMemo(
    () => [
      ...moduleFunctions.map((value) => itemIdentifier(value)),
      functionIdentifierValue("_other"),
    ],
    [moduleFunctions],
  );

  const defaultSelectedFunctions: MultiSelectValue = "all";

  const [selectedFunctions, setSelectedFunctions] =
    useGlobalLocalStorage<MultiSelectValue>(
      `logs/${deploymentPrefix}/selectedFunctions`,
      defaultSelectedFunctions,
    );

  const [logs, setLogs] = useState<UdfLog[]>([]);
  const [filteredLogs, setFilteredLogs] = useState<UdfLog[]>([]);
  const [pausedLogs, setPausedLogs] = useState<UdfLog[]>([]);
  const [pausedFilteredLogs, setPausedFilteredLogs] = useState<UdfLog[]>([]);

  const filters = useMemo(
    () => ({
      logTypes: levels,
      functions,
      selectedNents,
      selectedFunctions,
      filter,
    }),
    [filter, functions, levels, selectedFunctions, selectedNents],
  );
  const previousFilters = usePrevious(filters);

  const [clearedLogs, setClearedLogs] = useState<number[]>([]);

  const [fromTimestamp, setFromTimestamp] = useState<number>();
  const deploymentAuditLogs = useDeploymentAuditLogs(fromTimestamp);

  const receiveLogs = useCallback(
    (entries: UdfLog[], isPaused: boolean) => {
      if (isPaused) {
        // When paused, store new logs separately (except logs for existing requests)
        setPausedLogs((prev) => [...prev, ...entries]);
        const filteredEntries = filterLogs(filters, entries) || [];
        setPausedFilteredLogs((prev) => [...prev, ...filteredEntries]);
      } else {
        setLogs((prev) =>
          [...prev, ...entries].slice(
            Math.max(prev.length + entries.length - MAX_LOGS, 0),
            prev.length + entries.length,
          ),
        );
        setFilteredLogs((prev) => {
          const filteredEntries = filterLogs(filters, entries) || [];
          return [...prev, ...filteredEntries].slice(
            Math.max(prev.length + filteredEntries.length - MAX_LOGS, 0),
            prev.length + filteredEntries.length,
          );
        });
      }
    },
    [filters],
  );

  const [manuallyPaused, setManuallyPaused] = useState(false);
  const [paused, setPaused] = useState<number>(0);
  const onPause = (p: boolean) => {
    const now = new Date().getTime();
    setPaused(p ? now : 0);

    // When unpausing, merge pausedLogs into logs
    if (!p && pausedLogs.length > 0) {
      setLogs((prev) => {
        const combined = [...prev, ...pausedLogs];
        return combined.slice(
          Math.max(combined.length - MAX_LOGS, 0),
          combined.length,
        );
      });
      setFilteredLogs((prev) => {
        const combined = [...prev, ...pausedFilteredLogs];
        return combined.slice(
          Math.max(combined.length - MAX_LOGS, 0),
          combined.length,
        );
      });
      setPausedLogs([]);
      setPausedFilteredLogs([]);
    }
  };
  useLogs(
    logsConnectivityCallbacks.current,
    (entries) => receiveLogs(entries, paused > 0 || manuallyPaused),
    false, // Never skip the stream, always stay connected
  );

  useEffect(() => {
    if (isEqual(filters, previousFilters)) {
      return;
    }
    const newFilteredLogs = filterLogs(filters, logs) || [];
    setFilteredLogs(newFilteredLogs);
  }, [filters, previousFilters, logs]);

  const [innerFilter, setInnerFilter] = useState(filter);
  const pendingFilterRef = useRef(false);
  const pendingQuerySignatureRef = useRef<string | null>(null);
  const pendingQueryUpdateRef = useRef(false);
  const [queryUpdateVersion, setQueryUpdateVersion] = useState(0);
  const requestedQueryUpdateVersionRef = useRef(0);
  const markFiltersChanged = useCallback(() => {
    pendingQueryUpdateRef.current = true;
    requestedQueryUpdateVersionRef.current += 1;
    setQueryUpdateVersion(requestedQueryUpdateVersionRef.current);
  }, []);
  useDebounce(
    () => {
      setFilter(innerFilter);
      if (pendingFilterRef.current) {
        pendingFilterRef.current = false;
        markFiltersChanged();
      }
    },
    200,
    [innerFilter],
  );

  // Function to set filter that also updates the text input
  const setFilterAndInput = useCallback(
    (newFilter: string) => {
      pendingFilterRef.current = false;
      setFilter(newFilter);
      setInnerFilter(newFilter);
      markFiltersChanged();
    },
    [setFilter, markFiltersChanged],
  );

  const previousQuerySignatureRef = useRef(querySignature);
  useEffect(() => {
    const currentQuery: ParsedUrlQuery = JSON.parse(querySignature);
    const hasLogParams = querySignature !== "{}";
    const hadLogParams = previousQuerySignatureRef.current !== "{}";
    previousQuerySignatureRef.current = querySignature;
    // A completed URL update can arrive after the user has started the next edit.
    const isOwnUpdate = querySignature === pendingQuerySignatureRef.current;
    pendingQuerySignatureRef.current = null;
    if (isOwnUpdate) {
      return;
    }
    pendingQueryUpdateRef.current = false;
    const newFilter =
      typeof currentQuery.filter === "string"
        ? currentQuery.filter
        : currentQuery.filter === undefined && (hasLogParams || hadLogParams)
          ? ""
          : undefined;
    if (newFilter !== undefined) {
      pendingFilterRef.current = false;
      setFilter(newFilter);
      setInnerFilter(newFilter);
    }
    const selectionSetters = {
      components: setSelectedNents,
      functions: setSelectedFunctions,
      logTypes: setLevels,
    };
    for (const key of ["components", "functions", "logTypes"] as const) {
      const selection = parseLogSelection(
        currentQuery[key],
        key === "functions"
          ? isValidFunctionSelection
          : key === "logTypes"
            ? (option) =>
                [
                  "success",
                  "failure",
                  "DEBUG",
                  "INFO",
                  "WARN",
                  "ERROR",
                ].includes(option)
            : undefined,
      );
      if (selection !== undefined) {
        selectionSetters[key](selection);
      } else if (
        currentQuery[key] === undefined &&
        (hasLogParams || hadLogParams)
      ) {
        selectionSetters[key]("all");
      }
    }
  }, [
    querySignature,
    setFilter,
    setSelectedNents,
    setSelectedFunctions,
    setLevels,
  ]);

  // Initial URL selections take precedence; subsequent context changes follow the switcher.
  const previousContextPathRef = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    const path = selectedNent?.path ?? null;
    const previousPath = previousContextPathRef.current;
    previousContextPathRef.current = path;
    if (path === previousPath) return;
    const isInitialSeed = previousPath === undefined;
    if (isInitialSeed && (query.components !== undefined || path === null)) {
      return;
    }
    setSelectedNents(path === null ? "all" : [path]);
    if (!isInitialSeed) markFiltersChanged();
  }, [selectedNent, query.components, setSelectedNents, markFiltersChanged]);

  useEffect(() => {
    if (!router.isReady) return;
    const url = logsUrl(router, query);
    const newSignature = logsQuerySignature(url.query);
    const searchParams = new URLSearchParams(
      router.asPath.split("#")[0].split("?")[1],
    );
    const hasRedundantParams = [...searchParams.keys()].some(
      (key) => !(key in url.query),
    );
    if (newSignature === querySignature && !hasRedundantParams) return;
    if (newSignature !== querySignature) {
      pendingQuerySignatureRef.current = newSignature;
    }
    void router.replace(url, undefined, { shallow: true, scroll: false });
  }, [router, query, querySignature]);

  useEffect(() => {
    // Component switches can queue filter state from an effect; use its committed render.
    if (
      !pendingQueryUpdateRef.current ||
      queryUpdateVersion !== requestedQueryUpdateVersionRef.current
    )
      return;
    pendingQueryUpdateRef.current = false;
    const newQuery: ParsedUrlQuery = {
      ...query,
      components: serializeLogSelection(selectedNents),
      functions: serializeLogSelection(selectedFunctions),
      logTypes: serializeLogSelection(levels),
    };
    if (selectedNents === "all" && selectedNent) {
      delete newQuery.component;
    }
    if (filter) {
      newQuery.filter = filter;
    } else {
      delete newQuery.filter;
    }
    const url = logsUrl(router, newQuery);
    const newSignature = logsQuerySignature(url.query);
    if (
      newSignature === querySignature &&
      isEqual(url.query, logsUrl(router, query).query)
    )
      return;
    if (newSignature !== querySignature) {
      pendingQuerySignatureRef.current = newSignature;
    }
    void router.replace(url, undefined, {
      shallow: true,
      scroll: false,
    });
  }, [
    queryUpdateVersion,
    filter,
    levels,
    selectedNents,
    selectedNent,
    selectedFunctions,
    query,
    querySignature,
    router,
  ]);

  // Note: fromTimestamp used to be a `useMemo` result, but it was causing a bug
  // where fromTimestamp would keep changing and causing the query to be refetched
  // every time the first log entry changed
  // (which shouldn't happen, but I haven't debugged why that does happen yet).
  useEffect(() => {
    if (logs && logs[0] && fromTimestamp === undefined) {
      setFromTimestamp(logs[0].timestamp);
    }
  }, [logs, fromTimestamp]);

  const latestLog = logs?.at(-1);
  const latestAuditLog = deploymentAuditLogs?.at(-1);
  const latestTimestamp =
    (latestLog?.timestamp ?? 0) > (latestAuditLog?._creationTime ?? 0)
      ? latestLog?.timestamp
      : latestAuditLog?._creationTime;

  return (
    <div className="flex size-full min-w-3xl flex-col overflow-hidden p-6 py-4">
      <div className="flex shrink-0 flex-col gap-4">
        <LogToolbar
          firstItem={<LogsHeader />}
          selectedLevels={levels}
          selectedFunctions={selectedFunctions}
          setSelectedFunctions={(selection) => {
            setSelectedFunctions(selection);
            markFiltersChanged();
          }}
          functions={functions}
          setSelectedLevels={(selection) => {
            setLevels(selection);
            markFiltersChanged();
          }}
          nents={
            nents.length >= 1
              ? [NENT_APP_PLACEHOLDER, ...nents.map((nent) => nent.path)]
              : undefined
          }
          selectedNents={selectedNents}
          setSelectedNents={(selection) => {
            setSelectedNents(selection);
            markFiltersChanged();
          }}
        />
        <div className="mb-2 flex w-full gap-2">
          <SearchLogsInput
            value={innerFilter}
            onChange={(e) => {
              pendingFilterRef.current = true;
              setInnerFilter(e.target.value);
            }}
            logs={logs}
          />
          <Button
            size="sm"
            variant="neutral"
            tip="Clear the currently visible logs to declutter this page."
            tipSide="left"
            disabled={
              latestTimestamp === undefined ||
              !logs ||
              (clearedLogs.length
                ? logs.filter(
                    (log) =>
                      log.timestamp > clearedLogs[clearedLogs.length - 1],
                  )
                : logs
              ).length === 0
            }
            onClick={() => {
              setClearedLogs([...clearedLogs, latestTimestamp!]);
            }}
          >
            Clear Logs
          </Button>
        </div>
      </div>
      <div className="flex-1 overflow-hidden">
        <LogList
          logs={logs}
          pausedLogs={pausedLogs}
          filteredLogs={filteredLogs}
          deploymentAuditLogs={deploymentAuditLogs}
          setFilter={setFilterAndInput}
          clearedLogs={clearedLogs}
          setClearedLogs={setClearedLogs}
          paused={paused > 0 || manuallyPaused}
          setPaused={onPause}
          setManuallyPaused={(p) => {
            onPause(p);
            setManuallyPaused(p);
          }}
        />
      </div>
    </div>
  );
}

function LogsHeader() {
  return (
    <div className="mr-2 flex grow items-center justify-between gap-2">
      <div className="flex items-center gap-2">
        <h3>Logs</h3>
      </div>
    </div>
  );
}
