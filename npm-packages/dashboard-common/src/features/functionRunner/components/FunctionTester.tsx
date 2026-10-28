import {
  EnterFullScreenIcon,
  ExitFullScreenIcon,
  QuestionMarkCircledIcon,
  ViewHorizontalIcon,
  ViewVerticalIcon,
} from "@radix-ui/react-icons";
import classNames from "classnames";
import { BaseConvexClient, UserIdentityAttributes } from "convex/browser";
import { useQuery } from "convex/react";
import { ValidatorJSON, Value } from "convex/values";
import isEqual from "lodash/isEqual";
import { Link } from "@ui/Link";
import { useCallback, useContext, useState } from "react";
import { useDebounce } from "react-use";
import { ZodError, z } from "zod";
import { generateErrorMessage } from "zod-error";
import { UNDEFINED_PLACEHOLDER } from "system-udfs/convex/_system/frontend/lib/values";
import {
  itemIdentifier,
  useModuleFunctions,
} from "@common/lib/functions/FunctionsProvider";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { Combobox, Option } from "@ui/Combobox";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import {
  displayName,
  functionIdentifierFromValue,
  functionIdentifierValue,
} from "@common/lib/functions/generateFileTree";
import { ModuleFunction } from "@common/lib/functions/types";
import { defaultValueForValidator } from "@common/lib/defaultValueForValidator";
import { ObjectEditor } from "@common/elements/ObjectEditor/ObjectEditor";
import { NENT_APP_PLACEHOLDER, useNents } from "@common/lib/useNents";
import { NentNameOption } from "@common/elements/NentSwitcher";
import {
  CustomQuery,
  findFirstWritingFunction,
  findFunction,
  useGlobalRunnerSelectedItem,
  useHideGlobalRunner,
  useIsGlobalRunnerShown,
} from "@common/features/functionRunner/lib/functionRunner";
import { useFunctionEditor } from "@common/features/functionRunner/components/FunctionEditor";
import { useFunctionResult } from "@common/features/functionRunner/components/FunctionResult";
import { QueryResult } from "@common/features/functionRunner/components/QueryResult";
import {
  RunHistory,
  RunHistoryItem,
  useImpersonatedUser,
  useIsImpersonating,
} from "@common/features/functionRunner/components/RunHistory";
import { useGlobalReactClient } from "@common/features/functionRunner/lib/client";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";
import udfs from "@common/udfs";

const CUSTOM_TEST_QUERY_PLACEHOLDER =
  "__CONVEX_PLACEHOLDER_custom_test_query_1255035852";

// Claims are sent to the backend as JSON, so Convex-only values (bigints,
// bytes) can't be represented.
function isJsonValue(value: unknown): boolean {
  if (value === null || typeof value === "boolean") {
    return true;
  }
  if (typeof value === "number") {
    return Number.isFinite(value);
  }
  if (typeof value === "string") {
    return true;
  }
  if (Array.isArray(value)) {
    return value.every(isJsonValue);
  }
  if (
    typeof value === "object" &&
    Object.getPrototypeOf(value) === Object.prototype
  ) {
    return Object.values(value as Record<string, unknown>).every(isJsonValue);
  }
  return false;
}

const customClaimSchema = z.custom<any>(isJsonValue, {
  message: "custom claims must be JSON values",
});

const impersonatedUserSchema = z
  .object({
    subject: z.string(),
    issuer: z.string(),
    name: z.string().optional(),
    givenName: z.string().optional(),
    familyName: z.string().optional(),
    nickname: z.string().optional(),
    preferredUsername: z.string().optional(),
    profileUrl: z.string().optional(),
    pictureUrl: z.string().optional(),
    email: z.string().optional(),
    emailVerified: z.boolean().optional(),
    gender: z.string().optional(),
    birthday: z.string().optional(),
    timezone: z.string().optional(),
    language: z.string().optional(),
    phoneNumber: z.string().optional(),
    phoneNumberVerified: z.boolean().optional(),
    address: z.string().optional(),
    updatedAt: z.string().optional(),
    customClaims: z.record(customClaimSchema).optional(),
  })
  .catchall(customClaimSchema);

/** Throws a {@link ZodError} if `value` isn't a valid identity. */
export function parseImpersonatedUser(value: Value): UserIdentityAttributes {
  const { customClaims, ...rootClaims } = impersonatedUserSchema.parse(value);
  return {
    ...rootClaims,
    ...(customClaims || {}),
  };
}

const SUPPORTED_FUNCTION_TYPES = new Set(["Query", "Mutation", "Action"]);

export function GlobalFunctionTester({
  isVertical,
  setIsVertical,
  isExpanded,
  setIsExpanded,
  onRanCustomQuery,
  onCopiedQueryResult,
}: {
  isVertical: boolean;
  setIsVertical: (v: boolean) => void;
  isExpanded: boolean;
  setIsExpanded: (v: boolean) => void;
  onRanCustomQuery?: () => void;
  onCopiedQueryResult?: () => void;
}) {
  const isShowing = useIsGlobalRunnerShown();
  const hideGlobalRunner = useHideGlobalRunner();

  const [selectedItem, setSelectedItem] = useGlobalRunnerSelectedItem();

  const { nents } = useNents();
  const moduleFunctions = useModuleFunctions();

  let options: Option<string>[] = moduleFunctions
    .filter(
      (value) =>
        value.componentId === selectedItem?.componentId &&
        SUPPORTED_FUNCTION_TYPES.has(value.udfType),
    )
    .map((value) => ({
      // Since you choose the component in a separate combobox, we don't need
      // to show the component tooltip.
      label: functionIdentifierValue(value.identifier),
      value: itemIdentifier(value),
    }));

  const customTestQueryOption: Option<string> = {
    label: functionIdentifierValue("Custom test query"),
    value: functionIdentifierValue(
      CUSTOM_TEST_QUERY_PLACEHOLDER,
      undefined,
      selectedItem?.componentId ?? undefined,
    ),
  };
  options = [customTestQueryOption, ...options];

  const selectedFunction =
    selectedItem?.fn.type !== "customQuery"
      ? ((selectedItem &&
          // Get the most up to date version of this module. Important if the udf type changes.
          findFunction(
            moduleFunctions,
            selectedItem.fn.identifier,
            selectedItem.componentId,
          )) ??
        null)
      : null;

  const argsValidatorString = useQuery(
    udfs.modules.argsValidator,
    selectedFunction
      ? {
          componentId: selectedFunction.componentId,
          function: selectedFunction.displayName,
        }
      : "skip",
  );

  const argsValidator = argsValidatorString && JSON.parse(argsValidatorString);
  const initialArgs = argsValidator
    ? (defaultValueForValidator(argsValidator) as Record<string, Value>)
    : undefined;
  const [runHistoryItem, setRunHistoryItem] = useState<RunHistoryItem>();
  const { args, result, button } = useFunctionTester({
    moduleFunction: selectedFunction,
    initialArgs,
    argsValidator,
    runHistoryItem,
    setRunHistoryItem,
    onCopiedQueryResult,
  });
  const { queryEditor, customQueryResult, runCustomQueryButton } =
    useFunctionEditor(
      selectedItem?.fn.type === "customQuery" ? selectedItem.fn.table : null,
      selectedItem?.componentId ?? null,
      runHistoryItem,
      setRunHistoryItem,
      onRanCustomQuery,
    );

  const { useLogDeploymentEvent } = useContext(DeploymentInfoContext);
  const log = useLogDeploymentEvent();

  const logInfo = {
    function: selectedFunction !== null && {
      udfType: selectedFunction.udfType,
      visibility: selectedFunction?.visibility,
      identifier: selectedFunction.identifier,
    },
    customQuery: selectedItem?.fn.type === "customQuery",
  };

  return (
    <aside
      key={isVertical.toString() + isExpanded.toString()}
      className={classNames(
        "bg-background-secondary shadow-sm",
        "gap-6 z-30 relative overflow-auto",
        isExpanded
          ? "h-full w-full border-l"
          : isVertical
            ? "border-l flex-col h-full grow min-w-[32rem] max-w-[32rem] "
            : "min-h-[24rem] max-h-[24rem] border-t shrink",
        isShowing ? "flex" : "hidden",
      )}
    >
      <div
        className={classNames("absolute right-4 z-40 flex gap-1", "top-1.5")}
      >
        <Button
          size="xs"
          onClick={() => {
            log(
              `${isExpanded ? "collapse" : "expand"} function runner`,
              logInfo,
            );
            setIsExpanded(!isExpanded);
          }}
          inline
          className="bg-background-primary/50 backdrop-blur-[2px]"
          variant="neutral"
          icon={isExpanded ? <ExitFullScreenIcon /> : <EnterFullScreenIcon />}
          tip={isExpanded ? "Collapse" : "Expand"}
          tipSide={isExpanded || isVertical ? "bottom" : "top"}
        />
        <Button
          key={isVertical.toString()}
          tipSide={isVertical ? "bottom" : "top"}
          tip={`Align ${isVertical ? "horizontally" : "vertically"}`}
          size="xs"
          onClick={() => setIsVertical(!isVertical)}
          inline
          className="bg-background-primary/50 backdrop-blur-[2px]"
          variant="neutral"
          icon={isVertical ? <ViewHorizontalIcon /> : <ViewVerticalIcon />}
        />
        <Tooltip
          key={`close-${isVertical.toString()}`}
          side={isVertical ? "bottom" : "top"}
          tip="Close panel"
          asChild
        >
          <ClosePanelButton
            onClose={() => hideGlobalRunner("click")}
            className="bg-background-primary/50 backdrop-blur-[2px]"
          />
        </Tooltip>
      </div>
      <div className="flex size-full flex-col">
        <div
          className={classNames(
            "flex h-full w-full items-start",
            isVertical && "flex-col",
          )}
        >
          <div
            className={classNames(
              "flex flex-col gap-2 w-full",
              !isExpanded && "max-w-[40rem]",
              !isVertical && "border-r h-full",
            )}
          >
            <div className="sticky top-0 z-10 flex w-full items-center gap-4 border-y bg-background-primary px-4 py-2">
              <h4 className="text-xs whitespace-nowrap text-content-secondary">
                Function Input
              </h4>
            </div>
            <div
              className={classNames(
                "flex w-full flex-col gap-2 px-4 mt-4 min-w-[24rem]",
                isExpanded && "max-w-[32rem]",
              )}
            >
              {nents && nents.length > 1 && (
                <Combobox
                  buttonProps={{
                    tip: "Switch between components installed in this deployment.",
                    tipSide: "right",
                  }}
                  label="Select component"
                  className="w-full"
                  buttonClasses="w-full"
                  optionsWidth="full"
                  selectedOption={nents.find(
                    (nent) => nent.id === selectedItem?.componentId,
                  )}
                  unknownLabel={(nent) =>
                    nent
                      ? `Deleted component: ${nent.path}`
                      : `No component selected`
                  }
                  Option={NentNameOption}
                  setSelectedOption={(component) => {
                    const customQuery: CustomQuery = {
                      type: "customQuery",
                      table: null,
                    };
                    void setSelectedItem({
                      componentId: component?.id ?? null,
                      fn:
                        selectedItem?.fn.type === "customQuery"
                          ? customQuery
                          : findFirstWritingFunction(
                              moduleFunctions,
                              component?.id ?? null,
                            ) || customQuery,
                    });
                  }}
                  searchPlaceholder="Search components..."
                  options={[
                    {
                      label: NENT_APP_PLACEHOLDER,
                      value: undefined,
                    },
                    ...nents
                      .filter((n) => n.name !== null)
                      .map((nent) => ({
                        label: nent.path,
                        value: nent,
                      })),
                  ]}
                />
              )}
              <div className="flex w-full items-center gap-4">
                <Combobox
                  buttonProps={{
                    tip: "Select a function to run.",
                    tipSide: "right",
                  }}
                  label="Select function"
                  unknownLabel={(f) =>
                    `Deleted function: ${displayName(functionIdentifierFromValue(f).identifier)}`
                  }
                  className="w-full"
                  buttonClasses="w-full"
                  optionsWidth="full"
                  searchPlaceholder="Search functions..."
                  selectedOption={
                    selectedItem?.fn.type === "customQuery"
                      ? functionIdentifierValue(
                          CUSTOM_TEST_QUERY_PLACEHOLDER,
                          undefined,
                          selectedItem.componentId ?? undefined,
                        )
                      : selectedItem
                        ? itemIdentifier(selectedItem.fn)
                        : null
                  }
                  setSelectedOption={(option) => {
                    if (option === null) {
                      return;
                    }
                    const { identifier, componentId } =
                      functionIdentifierFromValue(option);
                    if (identifier === CUSTOM_TEST_QUERY_PLACEHOLDER) {
                      setSelectedItem({
                        componentId: componentId ?? null,
                        fn: {
                          type: "customQuery",
                          table: null,
                        },
                      });
                      return;
                    }
                    const fn = findFunction(
                      moduleFunctions,
                      identifier,
                      componentId ?? null,
                    );
                    if (fn !== undefined) {
                      setSelectedItem({ componentId: componentId ?? null, fn });
                    }
                  }}
                  options={options}
                  Option={(props) => (
                    <FunctionNameOption
                      {...{ ...props, disableTruncation: true }}
                    />
                  )}
                  processFilterOption={(option) => {
                    const id = functionIdentifierFromValue(option);
                    return id.componentPath
                      ? `${id.componentPath}/${id.identifier}`
                      : id.identifier;
                  }}
                />
              </div>
            </div>
            {!isVertical && (
              <>
                {selectedItem?.fn.type === "customQuery" ? (
                  <div className="mb-2 flex grow flex-col gap-4 px-4 pt-2">
                    {queryEditor}
                    {runCustomQueryButton}
                  </div>
                ) : selectedItem !== null ? (
                  <div className="mb-2 flex h-full flex-col gap-2 overflow-y-auto">
                    {args}
                    {button}
                  </div>
                ) : null}
              </>
            )}
          </div>
          {isVertical && (
            <>
              {selectedItem?.fn.type === "customQuery" ? (
                <div className="flex size-full flex-col gap-4 px-4 pt-4 pb-6">
                  {queryEditor}
                  {runCustomQueryButton}
                </div>
              ) : selectedItem !== null ? (
                <div className="flex h-fit w-full flex-col gap-2 pt-4 pb-6">
                  {args}
                  {button}
                </div>
              ) : null}
            </>
          )}
          <div
            className={classNames(
              "w-full h-full overflow-y-auto scrollbar max-w-full",
            )}
          >
            {selectedItem?.fn.type === "customQuery" ? (
              <div className="flex h-full">{customQueryResult}</div>
            ) : selectedItem !== null ? (
              <div className="flex h-full">{result}</div>
            ) : null}
          </div>
        </div>
      </div>
    </aside>
  );
}

// This is a hook because we want to return composable components that can be arranged
// vertically or horizontally.
export function useFunctionTester({
  moduleFunction,
  initialArgs,
  argsValidator,
  impersonation = true,
  runHistoryItem,
  setRunHistoryItem,
  onCopiedQueryResult,
}: {
  moduleFunction: ModuleFunction | null;
  initialArgs?: Record<string, Value>;
  argsValidator?: ValidatorJSON;
  impersonation?: boolean;
  runHistoryItem?: RunHistoryItem;
  setRunHistoryItem?: (item?: RunHistoryItem) => void;
  onCopiedQueryResult?: () => void;
}) {
  const [parameters, setParameters] = useState<Record<string, Value>>(
    initialArgs || {},
  );
  const [prevDefaults, setPrevDefaults] = useState([
    moduleFunction?.identifier,
    initialArgs,
  ]);

  // Reset the parameters when the function changes
  if (!isEqual([moduleFunction?.identifier, initialArgs], prevDefaults)) {
    setPrevDefaults([moduleFunction?.identifier, initialArgs]);
    setParameters(initialArgs || {});
  }

  const [hasError, setHasError] = useState(false);
  const [isInvalidObject, setIsInvalidObject] = useState(false);

  const onFirstParameterError = useCallback((errors: string[]) => {
    setHasError(!!errors.length);
  }, []);

  const [isImpersonating, setIsImpersonating] = useIsImpersonating();
  const [impersonatedUser, setImpersonatedUser] = useImpersonatedUser();
  const [impersonatedUserDebounced, setImpersonatedUserDebounced] =
    useState<UserIdentityAttributes>();

  const [impersonatedUserError, setImpersonatedUserError] = useState<string>();

  useDebounce(() => setImpersonatedUserDebounced(impersonatedUser), 200, [
    impersonatedUser,
  ]);

  const [reactClient] = useGlobalReactClient(
    isImpersonating ? impersonatedUserDebounced : undefined,
  );

  const onImpersonatedUserChange = useCallback(
    (v: Value) => {
      try {
        if (v === UNDEFINED_PLACEHOLDER) {
          setIsImpersonating(false);
          return;
        }
        setImpersonatedUser(parseImpersonatedUser(v));
        setImpersonatedUserError(undefined);
      } catch (e: any) {
        if (e instanceof ZodError) {
          setImpersonatedUserError(
            generateErrorMessage(e.issues, {
              delimiter: {
                error: " ",
                component: ", ",
              },
            }),
          );
        }
      }
    },
    [setImpersonatedUser, setIsImpersonating],
  );

  const onImpersonatedUserError = useCallback((errors: string[]) => {
    setImpersonatedUserError(
      errors.length
        ? "This user object is invalid. Fix the errors above to continue."
        : undefined,
    );
  }, []);

  const onChange = useCallback(
    (v: Value) => {
      if (v && Object.getPrototypeOf(v) === Object.prototype) {
        // Equals check to prevent infinite effect loops.
        if (!isEqual(v, parameters)) {
          // This cast is ok because of the protoype check above.
          setParameters(v as Record<string, Value>);
        }
        setIsInvalidObject(false);
      } else {
        setIsInvalidObject(true);
      }
    },
    [parameters],
  );
  const disabled = hasError || isInvalidObject || !!impersonatedUserError;

  const client = reactClient?.sync as BaseConvexClient;

  const onSubmit = useCallback(() => {
    if (!moduleFunction || !client) {
      return { requestFilter: null, runFunctionPromise: null };
    }
    const requestFilter = {
      clientRequestCounter: client.nextRequestId,
      sessionId: client.sessionId,
    };
    const { componentPath } = moduleFunction;

    const runFunctionPromise =
      moduleFunction.udfType === "Action"
        ? client.actionInternal(
            moduleFunction.displayName,
            parameters,
            componentPath ?? undefined,
          )
        : client.mutationInternal(
            moduleFunction.displayName,
            parameters,
            {},
            componentPath ?? undefined,
          );

    return {
      requestFilter,
      runFunctionPromise,
    };
  }, [client, moduleFunction, parameters]);

  const isInComponent = !!moduleFunction?.componentPath;
  const { button, result: functionResult } = useFunctionResult({
    onSubmit,
    disabled,
    udfType: moduleFunction?.udfType,
    visibility: moduleFunction?.visibility,
    isInComponent,
    functionIdentifier: moduleFunction?.identifier,
    componentId: moduleFunction?.componentId || null,
    args: parameters,
    runHistoryItem,
    onCopiedQueryResult,
  });

  const { useLogDeploymentEvent, useHasCustomRole } = useContext(
    DeploymentInfoContext,
  );
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canRunInternalQueries = useIsOperationAllowed("RunInternalQueries");
  const canViewData = useIsOperationAllowed("ViewData");
  const canActAsUser = useIsOperationAllowed("ActAsUser");
  const hasCustomRole = useHasCustomRole();

  const canRunQuery = (() => {
    if (!moduleFunction || moduleFunction.udfType !== "Query") return true;
    if (moduleFunction.visibility?.kind === "internal")
      return canRunInternalQueries;
    if (moduleFunction.componentPath) return canViewData;
    return true;
  })();

  const queryResult = moduleFunction &&
    reactClient &&
    moduleFunction.udfType === "Query" &&
    canRunQuery && (
      <QueryResult
        paused={disabled}
        module={moduleFunction}
        parameters={parameters}
        reactClient={reactClient}
        onCopiedQueryResult={onCopiedQueryResult}
      />
    );

  // Which custom-role action unblocks this query — surfaced inline so
  // members see exactly which grant is missing.
  const missingQueryAction = moduleFunction
    ? moduleFunction.visibility?.kind === "internal"
      ? ("deployment:functions:runInternalQueries" as const)
      : moduleFunction.componentPath
        ? ("deployment:data:view" as const)
        : null
    : null;
  const queryPermissionDenied = moduleFunction &&
    moduleFunction.udfType === "Query" &&
    !canRunQuery && (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-4 text-center">
        <p className="text-sm text-content-secondary">
          You do not have permission to run this function in this deployment.
        </p>
        {missingQueryAction && hasCustomRole && (
          <p className="text-xs text-content-tertiary">
            Missing permission:{" "}
            <code className="rounded-sm bg-background-tertiary px-1 py-0.5 font-mono">
              {missingQueryAction}
            </code>
          </p>
        )}
      </div>
    );
  const log = useLogDeploymentEvent();

  const args = (
    <div className="scrollbar grid h-full gap-2 overflow-y-auto">
      <div className="px-4">
        <div className="flex max-w-3xl items-end justify-between">
          <h5 className="text-xs text-content-secondary">Arguments</h5>
          {setRunHistoryItem && moduleFunction?.udfType !== "Query" && (
            <RunHistory
              functionIdentifier={moduleFunction?.identifier || ""}
              componentId={moduleFunction?.componentId ?? null}
              selectItem={(item) => {
                if (!item.type || item.type === "arguments") {
                  onChange(item.arguments);
                }
                setRunHistoryItem(item);
              }}
            />
          )}
        </div>
      </div>
      <div className="relative min-h-28 grow overflow-y-auto px-4">
        {argsValidator !== undefined && (
          <ObjectEditor
            className="max-w-3xl animate-fadeInFromLoading"
            key={
              (moduleFunction?.identifier || "") +
              (moduleFunction?.componentId || "") +
              (runHistoryItem?.startedAt.toString() || "")
            }
            fullHeight
            defaultValue={parameters}
            onChange={onChange}
            path={`arguments-${moduleFunction?.identifier}-${moduleFunction?.componentId}`}
            onError={onFirstParameterError}
            validator={argsValidator}
            shouldSurfaceValidatorErrors
            showTableNames
            mode="editDocument"
          />
        )}
      </div>
      {impersonation && (
        <div className="flex items-start gap-4 px-4">
          <Tooltip
            tip={
              !canActAsUser ? (
                <PermissionDeniedTip
                  message="You do not have permission to act as a user in this deployment."
                  action="deployment:functions:actAsUser"
                />
              ) : undefined
            }
          >
            <label
              htmlFor="actAsUser"
              className={classNames(
                "flex h-9 items-center gap-2 pt-0.5 text-xs whitespace-nowrap accent-util-accent",
                !canActAsUser && "opacity-50 cursor-not-allowed",
              )}
            >
              <input
                data-testid="actAsUser"
                id="actAsUser"
                type="checkbox"
                checked={isImpersonating && canActAsUser}
                disabled={!canActAsUser}
                className={classNames(
                  "hover:cursor-pointer",
                  !canActAsUser && "cursor-not-allowed",
                )}
                onChange={() => {
                  if (!canActAsUser) return;
                  setIsImpersonating(!isImpersonating);
                  setRunHistoryItem?.(undefined);
                  log("toggle act as user", {
                    actAsUser: isImpersonating,
                    function: moduleFunction && {
                      udfType: moduleFunction.udfType,
                      visibility: moduleFunction.visibility,
                      identifier: moduleFunction.identifier,
                    },
                  });
                }}
              />
              <span className="flex gap-1 select-none">
                Act as a user{" "}
                <Tooltip
                  tip={
                    <>
                      Run authenticated functions by acting as a user.{" "}
                      <Link
                        href="https://docs.convex.dev/dashboard/deployments/functions#assuming-a-user-identity"
                        passHref
                        target="_blank"
                      >
                        Learn more
                      </Link>
                      .
                    </>
                  }
                >
                  <QuestionMarkCircledIcon />
                </Tooltip>
              </span>
            </label>
          </Tooltip>
          <div className="flex max-h-32 w-full flex-col gap-1">
            {isImpersonating && (
              <ObjectEditor
                key={runHistoryItem?.startedAt || ""}
                className="w-full max-w-3xl animate-fadeInFromLoading"
                defaultValue={
                  runHistoryItem?.type === "arguments"
                    ? runHistoryItem.user
                    : impersonatedUser
                }
                disableFind
                onChange={onImpersonatedUserChange}
                onError={onImpersonatedUserError}
                path={`userAuth${runHistoryItem?.startedAt || ""}`}
                mode="editField"
              />
            )}
            {impersonatedUserError && (
              <p
                className="mt-1 h-4 text-xs wrap-break-word text-content-errorSecondary"
                role="alert"
              >
                {impersonatedUserError}
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );

  return {
    args,
    result: functionResult || queryResult || queryPermissionDenied,
    button,
  };
}
