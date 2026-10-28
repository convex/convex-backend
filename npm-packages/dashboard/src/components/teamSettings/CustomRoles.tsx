import { TeamResponse } from "generatedApi";
import {
  CustomRoleResponse,
  RoleStatement,
} from "@convex-dev/platform/managementApi";
import { Sheet } from "@ui/Sheet";
import { Callout } from "@ui/Callout";
import { Loading } from "@ui/Loading";
import { Button } from "@ui/Button";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { TextInput } from "@ui/TextInput";
import { Menu, MenuItem } from "@ui/Menu";
import {
  PlusIcon,
  DotsVerticalIcon,
  CheckCircledIcon,
} from "@radix-ui/react-icons";
import { KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/router";
import { Link } from "@ui/Link";
import { cn } from "@ui/cn";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { useTeamEntitlements } from "api/teams";
import {
  useListCustomRoles,
  useCreateCustomRole,
  useUpdateCustomRole,
  useDeleteCustomRole,
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { CUSTOM_ROLE_RESOURCE } from "lib/permissions";
import Editor, { BeforeMount, OnMount } from "@monaco-editor/react";
import { editorOptions } from "@common/elements/ObjectEditor/ObjectEditor";
import { useCurrentTheme } from "@common/lib/useCurrentTheme";
import { ActionCategory, ACTIONS_BY_CATEGORY } from "./customRoleActions";
import {
  CUSTOM_ROLE_TEMPLATES,
  CUSTOM_ROLE_TEMPLATES_BY_ID,
} from "./customRoleTemplates";
import { registerCustomRoleCompletions } from "./customRoleCompletions";

const BLANK_STATEMENT: RoleStatement = {
  effect: "allow",
  resource: "",
  actions: [],
};

const STATEMENTS_EDITOR_PATH = "custom-role-statements.json";

const STATEMENTS_PRINT_WIDTH = 160;
const STATEMENTS_INDENT = "  ";

function formatStatements(value: unknown): string {
  return formatCompact(value, "");
}

function inlineFormat(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(inlineFormat).join(", ")}]`;
  }
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>);
    if (entries.length === 0) return "{}";
    return `{ ${entries
      .map(([k, v]) => `${JSON.stringify(k)}: ${inlineFormat(v)}`)
      .join(", ")} }`;
  }
  return JSON.stringify(value) ?? "null";
}

function formatCompact(value: unknown, currentIndent: string): string {
  const inline = inlineFormat(value);
  if (currentIndent.length + inline.length <= STATEMENTS_PRINT_WIDTH) {
    return inline;
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return "[]";
    const childIndent = currentIndent + STATEMENTS_INDENT;
    const items = value.map((v) => childIndent + formatCompact(v, childIndent));
    return `[\n${items.join(",\n")}\n${currentIndent}]`;
  }
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>);
    if (entries.length === 0) return "{}";
    const childIndent = currentIndent + STATEMENTS_INDENT;
    const items = entries.map(
      ([k, v]) =>
        `${childIndent}${JSON.stringify(k)}: ${formatCompact(v, childIndent)}`,
    );
    return `{\n${items.join(",\n")}\n${currentIndent}}`;
  }
  return inline;
}

function errorMessage(e: unknown, fallback: string): string {
  if (
    typeof e === "object" &&
    e !== null &&
    "message" in e &&
    typeof (e as { message: unknown }).message === "string"
  ) {
    return (e as { message: string }).message;
  }
  return fallback;
}

const actionsForCategory = (category: ActionCategory) => ({
  if: { type: "array" },
  then: {
    minItems: 1,
    // Validating via `pattern` rather than `enum` so the diagnostic stays
    // compact. Monaco's enum error inlines the full list of allowed values
    // ("value `foo` is not allowed. Allowed values: …"), which is unreadable
    // for action lists with 60+ entries — IntelliSense already surfaces the
    // valid options, so a short message + the per-action docs there is enough.
    items: {
      type: "string",
      pattern: `^(${ACTIONS_BY_CATEGORY[category].join("|")})$`,
      patternErrorMessage: "Not a valid action for this resource.",
    },
  },
  else: { const: "*" },
});

// Tokens nest directly under their owning resource: `team:*:token:*`,
// `project:*:token:*`, or `project:*:deployment:*:token:*`.
const SELECTOR_VAL = "[^,:]+";
// `creator=` accepts the literal `self` (resolves to the evaluating actor)
// or a numeric member id. Tighter than `SELECTOR_VAL` so typos like
// `creator=me` get flagged in the editor instead of failing on save.
const CREATOR_VAL = "(self|[0-9]+)";
// Mirror the backend `DeploymentType` enum (camelCase serde) — `type=` only
// accepts one of these four values, so typos like `type=production` get
// flagged in the editor instead of failing on save.
const DEPLOYMENT_TYPE_VAL = "(dev|prod|preview|custom)";
const projectSel = `(\\*|id=${SELECTOR_VAL}|slug=${SELECTOR_VAL})`;
const deploymentSel = `(\\*|id=${SELECTOR_VAL}|type=${DEPLOYMENT_TYPE_VAL}|creator=${CREATOR_VAL})`;
const tokenSel = `(\\*|creator=${CREATOR_VAL})`;
const csv = (sel: string) => `${sel}(,${sel})*`;
const tokenTail = `:token:${csv(tokenSel)}`;
const projectTail = `(${tokenTail}|:deployment:${csv(deploymentSel)}(${tokenTail})?|:defaultEnvironmentVariable:\\*)`;
const RESOURCE_PATTERN =
  `^(team:\\*(${tokenTail})?` +
  `|project:${csv(projectSel)}${projectTail}?` +
  `|member:\\*` +
  `|customRole:\\*` +
  `|billing:\\*` +
  `|oauthApplication:\\*` +
  `|sso:\\*` +
  `|directorySync:\\*` +
  `|integration:\\*)$`;

// ⚠️ If updating this, please remember to also update the
//    JSON schema in docs (npm-packages/docs/docs/team-management/custom-roles.mdx)
const statementsSchema = {
  $schema: "http://json-schema.org/draft-07/schema#",
  title: "Custom Role Statements",
  type: "array",
  items: { $ref: "#/definitions/statement" },
  definitions: {
    statement: {
      type: "object",
      required: ["effect", "actions", "resource"],
      additionalProperties: false,
      properties: {
        effect: { enum: ["allow", "deny"] },
        actions: { $ref: "#/definitions/actionPattern" },
        resource: { $ref: "#/definitions/resourceSpecifier" },
      },
      allOf: [
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^team:[^:]+$" } },
          },
          then: { properties: { actions: actionsForCategory("team") } },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^project:[^:]+$" } },
          },
          then: { properties: { actions: actionsForCategory("project") } },
        },
        {
          if: {
            required: ["resource"],
            properties: {
              resource: { pattern: "^project:[^:]+:deployment:[^:]+$" },
            },
          },
          then: {
            properties: { actions: actionsForCategory("deployment") },
          },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^member:[^:]+$" } },
          },
          then: { properties: { actions: actionsForCategory("member") } },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^team:[^:]+:token:[^:]+$" } },
          },
          then: { properties: { actions: actionsForCategory("teamToken") } },
        },
        {
          if: {
            required: ["resource"],
            properties: {
              resource: { pattern: "^project:[^:]+:token:[^:]+$" },
            },
          },
          then: {
            properties: { actions: actionsForCategory("projectToken") },
          },
        },
        {
          if: {
            required: ["resource"],
            properties: {
              resource: {
                pattern: "^project:[^:]+:deployment:[^:]+:token:[^:]+$",
              },
            },
          },
          then: {
            properties: { actions: actionsForCategory("deploymentToken") },
          },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^customRole:[^:]+$" } },
          },
          then: { properties: { actions: actionsForCategory("customRole") } },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^billing:[^:]+$" } },
          },
          then: { properties: { actions: actionsForCategory("billing") } },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^oauthApplication:[^:]+$" } },
          },
          then: {
            properties: { actions: actionsForCategory("oauthApplication") },
          },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^sso:[^:]+$" } },
          },
          then: { properties: { actions: actionsForCategory("sso") } },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^directorySync:[^:]+$" } },
          },
          then: {
            properties: { actions: actionsForCategory("directorySync") },
          },
        },
        {
          if: {
            required: ["resource"],
            properties: { resource: { pattern: "^integration:[^:]+$" } },
          },
          then: { properties: { actions: actionsForCategory("integration") } },
        },
        {
          if: {
            required: ["resource"],
            properties: {
              resource: {
                pattern: "^project:[^:]+:defaultEnvironmentVariable:[^:]+$",
              },
            },
          },
          then: {
            properties: {
              actions: actionsForCategory("defaultEnvironmentVariable"),
            },
          },
        },
      ],
    },
    // Structural shape only — the per-resource-kind narrowing in `statement`
    // owns the action enum check, so listing the enum here too would surface a
    // duplicate "value not allowed" tooltip for unknown actions.
    actionPattern: {
      if: { type: "array" },
      then: {
        minItems: 1,
        uniqueItems: true,
        items: { type: "string" },
      },
      else: { const: "*" },
    },
    resourceSpecifier: {
      type: "string",
      minLength: 1,
      pattern: RESOURCE_PATTERN,
      patternErrorMessage: "Invalid resource specifier.",
    },
  },
};

function CustomRoleForm({
  teamId,
  teamSlug,
  existingRole,
  templateId,
  readOnly = false,
  onClose,
  onSaved,
}: {
  teamId: number;
  teamSlug: string;
  existingRole?: CustomRoleResponse;
  templateId?: string;
  readOnly?: boolean;
  onClose: () => void;
  onSaved: (roleId: number) => void;
}) {
  const template =
    !existingRole && templateId
      ? CUSTOM_ROLE_TEMPLATES_BY_ID[templateId]
      : undefined;
  const [name, setName] = useState(
    existingRole?.name ?? template?.defaultName ?? "",
  );
  const [description, setDescription] = useState(
    existingRole?.description ?? template?.defaultRoleDescription ?? "",
  );
  const [statementsText, setStatementsText] = useState(() => {
    const initial = existingRole?.statements ??
      template?.statements ?? [BLANK_STATEMENT];
    return formatStatements(initial);
  });
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string>();
  const [nameError, setNameError] = useState<string>();
  const [hasSchemaError, setHasSchemaError] = useState(false);
  const [savedRoleName, setSavedRoleName] = useState<string>();
  // Tracks the id of a role created in this form instance so that a fast
  // second save after Create still routes through update, even if the SWR
  // cache for `list_custom_roles` hasn't yet repopulated `existingRole`.
  const [createdRoleId, setCreatedRoleId] = useState<number | undefined>();
  const savedRoleId = existingRole?.id ?? createdRoleId;
  const currentTheme = useCurrentTheme();
  const prefersDark = currentTheme === "dark";

  // A dedicated DOM node attached to `<body>` that hosts Monaco's overflow
  // widgets (suggest popup, hover, parameter hints). The form's slide-in
  // animation puts the editor inside a CSS `transform` subtree, which
  // establishes a new containing block for `position: fixed` — so
  // `fixedOverflowWidgets` alone still clips the popup. Attaching widgets to a
  // body-level node sidesteps the transformed ancestor entirely. We render the
  // editor only after the node exists so Monaco picks it up at construction.
  const [overflowWidgetsNode, setOverflowWidgetsNode] =
    useState<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = document.createElement("div");
    // `monaco-editor` class makes Monaco's widget CSS apply inside this node.
    el.className = "monaco-editor";
    el.style.position = "absolute";
    el.style.top = "0";
    el.style.left = "0";
    el.style.zIndex = "1000";
    document.body.appendChild(el);
    setOverflowWidgetsNode(el);
    return () => {
      el.remove();
    };
  }, []);

  const createCustomRole = useCreateCustomRole(teamId);
  const updateCustomRole = useUpdateCustomRole(teamId);

  // Synchronous validation, derived from `statementsText`. `hasSchemaError`
  // reflects Monaco markers, which update asynchronously after typing — a
  // quick Save before markers settle would otherwise slip past every gate and
  // only fail inside `handleSubmit`. Keep this in sync with the checks there.
  const statementsValidationError = useMemo<string | undefined>(() => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(statementsText);
    } catch {
      return "Statements must be valid JSON.";
    }
    if (!Array.isArray(parsed)) {
      return "Statements must be a JSON array.";
    }
    if (parsed.length === 0) {
      return "A custom role must have at least one statement.";
    }
    for (const stmt of parsed) {
      if (
        stmt !== null &&
        typeof stmt === "object" &&
        Array.isArray((stmt as { actions?: unknown }).actions)
      ) {
        const actions = (stmt as { actions: unknown[] }).actions;
        const seen = new Set<unknown>();
        for (const a of actions) {
          if (seen.has(a)) {
            return `Duplicate action ${JSON.stringify(a)} in a statement.`;
          }
          seen.add(a);
        }
      }
    }
    return undefined;
  }, [statementsText]);
  const saveBlockedReason = statementsValidationError
    ? statementsValidationError
    : hasSchemaError
      ? "Resolve the highlighted schema errors first."
      : undefined;
  const canSave = !isSubmitting && saveBlockedReason === undefined;

  const handleEditorBeforeMount: BeforeMount = (monaco) => {
    monaco.languages.json.jsonDefaults.setDiagnosticsOptions({
      validate: true,
      allowComments: false,
      // Monaco defaults schema-validation and trailing-comma findings to
      // Warning severity, but we treat them as hard errors: the marker check
      // below only blocks saving on severity-Error markers, and `JSON.parse`
      // rejects trailing commas anyway.
      schemaValidation: "error",
      trailingCommas: "error",
      comments: "error",
      schemas: [
        {
          uri: STATEMENTS_EDITOR_PATH,
          fileMatch: [STATEMENTS_EDITOR_PATH],
          schema: statementsSchema,
        },
      ],
    });
    // Turn off the JSON service's built-in completions so they don't duplicate
    // the richer items from `registerCustomRoleCompletions` (the schema-driven
    // ones surface plain enum values with no docs and a different item-kind
    // badge). Validation/hovers/etc. live on `setDiagnosticsOptions` above and
    // aren't affected. The dashboard's other Monaco editors use JavaScript, so
    // this global setting only impacts the statements editor.
    monaco.languages.json.jsonDefaults.setModeConfiguration({
      ...monaco.languages.json.jsonDefaults.modeConfiguration,
      completionItems: false,
    });
  };

  const handleSubmitRef = useRef<() => void>(() => {});

  const handleEditorMount: OnMount = (editorInstance, monaco) => {
    editorInstance.addAction({
      id: "saveCustomRole",
      label: "Save custom role",
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
      run() {
        handleSubmitRef.current();
      },
    });

    const model = editorInstance.getModel();
    if (!model) return;

    const completionsDisposable = registerCustomRoleCompletions(
      monaco,
      model.uri.toString(),
    );
    const updateMarkers = () => {
      const markers = monaco.editor.getModelMarkers({ resource: model.uri });
      setHasSchemaError(
        markers.some((m) => m.severity === monaco.MarkerSeverity.Error),
      );
    };
    updateMarkers();
    const markersDisposable = monaco.editor.onDidChangeMarkers((uris) => {
      if (uris.some((u) => u.toString() === model.uri.toString())) {
        updateMarkers();
      }
    });

    const denyDecorations = editorInstance.createDecorationsCollection();
    const updateDenyDecorations = () => {
      const matches = model.findMatches(
        '"deny"',
        false,
        false,
        true,
        null,
        false,
      );
      denyDecorations.set(
        matches.map((match) => ({
          range: new monaco.Range(
            match.range.startLineNumber,
            match.range.startColumn + 1,
            match.range.endLineNumber,
            match.range.endColumn - 1,
          ),
          options: { inlineClassName: "customRoleDenyHighlight" },
        })),
      );
    };
    updateDenyDecorations();
    const contentDisposable = model.onDidChangeContent(() => {
      updateDenyDecorations();
    });

    editorInstance.onDidDispose(() => {
      markersDisposable.dispose();
      contentDisposable.dispose();
      completionsDisposable.dispose();
    });
  };

  const handleSubmit = async () => {
    setError(undefined);
    setNameError(undefined);
    setSavedRoleName(undefined);

    if (!name.trim()) {
      setNameError("Name is required.");
      return;
    }

    if (saveBlockedReason !== undefined) {
      setError(saveBlockedReason);
      return;
    }

    // `saveBlockedReason` already gated parseability, array shape, and
    // non-emptiness, so this parse can't throw and the cast is safe.
    const parsedStatements = JSON.parse(statementsText) as RoleStatement[];

    setIsSubmitting(true);
    try {
      const trimmedName = name.trim();
      if (savedRoleId !== undefined) {
        await updateCustomRole({
          id: savedRoleId,
          name: trimmedName,
          description: description.trim() || null,
          statements: parsedStatements,
        });
        setSavedRoleName(trimmedName);
        onSaved(savedRoleId);
      } else {
        const created = await createCustomRole({
          name: trimmedName,
          description: description.trim() || null,
          statements: parsedStatements,
        });
        if (created) {
          setCreatedRoleId(created.id);
          setSavedRoleName(trimmedName);
          onSaved(created.id);
        }
      }
    } catch (e: unknown) {
      setError(errorMessage(e, "An error occurred."));
    } finally {
      setIsSubmitting(false);
    }
  };

  useEffect(() => {
    handleSubmitRef.current = () => {
      if (!canSave) return;
      void handleSubmit();
    };
  });

  const handleCmdEnter = (
    e: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>,
  ) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      handleSubmitRef.current();
    }
  };

  return (
    <Sheet className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1 flex-col gap-4">
        <TextInput
          id="role-name"
          label="Name"
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setNameError(undefined);
            setSavedRoleName(undefined);
          }}
          onKeyDown={handleCmdEnter}
          placeholder="e.g. Viewer"
          error={nameError}
          disabled={readOnly}
        />
        <div className="flex flex-col gap-1">
          <label
            className="text-left text-sm text-content-primary"
            htmlFor="role-description"
          >
            Description
          </label>
          <textarea
            id="role-description"
            className="min-h-[60px] w-full rounded-sm border bg-background-secondary px-3 py-2 text-sm text-content-primary disabled:cursor-not-allowed disabled:bg-background-tertiary disabled:text-content-secondary"
            value={description}
            onChange={(e) => {
              setDescription(e.target.value);
              setSavedRoleName(undefined);
            }}
            onKeyDown={handleCmdEnter}
            placeholder="Optional description for this role"
            disabled={readOnly}
          />
        </div>
        <div className="flex min-h-0 flex-1 flex-col gap-1">
          <div className="text-left text-sm text-content-primary">
            Statements
          </div>
          <div className="min-h-48 flex-1 overflow-hidden rounded-sm border">
            {overflowWidgetsNode && (
              <Editor
                path={STATEMENTS_EDITOR_PATH}
                value={statementsText}
                language="json"
                theme={prefersDark ? "vs-dark" : "light"}
                onChange={(v) => {
                  setStatementsText(v ?? "");
                  setSavedRoleName(undefined);
                }}
                beforeMount={handleEditorBeforeMount}
                onMount={handleEditorMount}
                options={{
                  ...editorOptions,
                  readOnly,
                  scrollBeyondLastLine: false,
                  padding: { top: 8, bottom: 8 },
                  // Shared `editorOptions` disables IntelliSense; re-enable it
                  // for this editor so the schema + custom completion provider
                  // can surface action/resource/effect suggestions.
                  quickSuggestions: {
                    other: true,
                    comments: false,
                    strings: true,
                  },
                  suggestOnTriggerCharacters: true,
                  snippetSuggestions: "inline",
                  tabCompletion: "on",
                  // Render suggestion/hover popups in a fixed-position container
                  // outside the editor DOM so they aren't clipped. The form's
                  // slide-in animation puts the editor inside a `transform`
                  // ancestor, which establishes a new containing block for
                  // `position: fixed` — so `fixedOverflowWidgets` alone isn't
                  // enough. We additionally attach widgets to a portal'd div on
                  // `document.body` (`overflowWidgetsRef` below).
                  fixedOverflowWidgets: true,
                  overflowWidgetsDomNode: overflowWidgetsNode,
                  suggest: { preview: true, showWords: false },
                }}
              />
            )}
          </div>
        </div>
        <div className="flex w-full items-start justify-end gap-2">
          {!readOnly && error && (
            <p className="mr-auto text-sm text-content-errorSecondary">
              {error}
            </p>
          )}
          {!readOnly && !error && savedRoleName && (
            <div className="mr-auto flex items-start gap-1 text-sm">
              <CheckCircledIcon className="mt-0.5 shrink-0 text-content-success" />
              <div className="flex flex-col gap-1">
                <p>
                  Saved “{savedRoleName}”. Assign this role to a team member on
                  the{" "}
                  <Link
                    href={{
                      pathname: "/t/[team]/settings/members",
                      query: { team: teamSlug },
                    }}
                  >
                    Team Settings → Members page
                  </Link>
                  .
                </p>
                <p className="text-xs text-content-secondary">
                  It may take a few minutes to propogate deployment-level
                  permissions to deployments.
                </p>
              </div>
            </div>
          )}
          <Button variant="neutral" onClick={onClose} disabled={isSubmitting}>
            {readOnly ? "Close" : "Cancel"}
          </Button>
          {!readOnly && (
            <Button
              onClick={handleSubmit}
              disabled={!canSave}
              tip={saveBlockedReason}
            >
              {savedRoleId !== undefined ? "Save" : "Create"}
            </Button>
          )}
        </div>
      </div>
    </Sheet>
  );
}

function CustomRoleListItem({
  role,
  teamId,
  onView,
  onEdit,
  disabled,
  disabledReason,
}: {
  role: CustomRoleResponse;
  teamId: number;
  onView: () => void;
  onEdit: () => void;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const [showDelete, setShowDelete] = useState(false);
  const [deleteError, setDeleteError] = useState<string>();
  const deleteCustomRole = useDeleteCustomRole(teamId);

  return (
    <div className="flex w-full flex-col py-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-col gap-0.5">
          <div>{role.name}</div>
          {role.description && (
            <div className="text-xs text-content-secondary">
              {role.description}
            </div>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-4">
          <div className="flex flex-col items-end">
            <div className="text-xs text-content-secondary">
              {role.statements.length} statement
              {role.statements.length !== 1 ? "s" : ""}
            </div>
            <TimestampDistance
              prefix="Created "
              date={new Date(role.createTime)}
            />
          </div>
          <Menu
            placement="bottom-end"
            buttonProps={{
              variant: "neutral",
              size: "xs",
              icon: <DotsVerticalIcon />,
              "aria-label": "Custom role options",
            }}
          >
            <MenuItem action={onView}>View</MenuItem>
            <MenuItem
              action={onEdit}
              disabled={disabled}
              tip={disabled ? disabledReason : undefined}
              tipSide="left"
            >
              Edit
            </MenuItem>
            <MenuItem
              variant="danger"
              action={() => setShowDelete(true)}
              disabled={disabled}
              tip={disabled ? disabledReason : undefined}
              tipSide="left"
            >
              Delete
            </MenuItem>
          </Menu>
        </div>
      </div>
      {showDelete && (
        <ConfirmationDialog
          dialogTitle="Delete Custom Role"
          dialogBody={`Are you sure you want to delete the custom role "${role.name}"? This cannot be undone.`}
          validationText={role.name}
          confirmText="Delete"
          error={deleteError}
          onClose={() => {
            setShowDelete(false);
            setDeleteError(undefined);
          }}
          onConfirm={async () => {
            try {
              await deleteCustomRole({ id: role.id });
              setShowDelete(false);
            } catch (e: unknown) {
              setDeleteError(errorMessage(e, "Failed to delete."));
            }
          }}
        />
      )}
    </div>
  );
}

export function CustomRoles({ team }: { team: TeamResponse }) {
  const canViewCustomRoles = useHasCustomRolePermission(
    team.id,
    "customRole:view",
    CUSTOM_ROLE_RESOURCE,
    true,
  );

  if (canViewCustomRoles === false) {
    return (
      <>
        <h2>Custom Roles</h2>
        <NoPermissionMessage
          message="You do not have permission to view custom roles."
          missingPermission="customRole:view"
        />
      </>
    );
  }

  return <CustomRolesContent team={team} />;
}

function CustomRolesContent({ team }: { team: TeamResponse }) {
  const router = useRouter();
  const entitlements = useTeamEntitlements(team.id);
  const entitlementsLoaded = entitlements !== undefined;
  const customRolesEnabled = entitlements?.customRolesEnabled ?? false;
  const hasAdminPermissions = useIsCurrentMemberTeamAdmin();
  const { data: customRolesData } = useListCustomRoles(
    customRolesEnabled ? team.id : undefined,
  );
  const roles = customRolesEnabled ? customRolesData?.items : [];

  const canManage = customRolesEnabled && hasAdminPermissions;
  const disabledReason = !entitlementsLoaded
    ? undefined
    : !customRolesEnabled
      ? "Custom roles are not enabled for this team."
      : !hasAdminPermissions
        ? "You do not have permission to manage custom roles."
        : undefined;

  const editingRoleId =
    typeof router.query.role === "string" ? router.query.role : undefined;
  const editingRole = editingRoleId
    ? roles?.find((r) => r.id.toString() === editingRoleId)
    : undefined;
  const viewingRoleId =
    typeof router.query.view === "string" ? router.query.view : undefined;
  const viewingRole = viewingRoleId
    ? roles?.find((r) => r.id.toString() === viewingRoleId)
    : undefined;
  const isNewRole = router.query.new === "1";
  const newRoleTemplateId =
    typeof router.query.template === "string"
      ? router.query.template
      : undefined;
  const showEditForm = canManage && (isNewRole || editingRoleId !== undefined);
  const showViewForm = viewingRoleId !== undefined;
  const showForm = showEditForm || showViewForm;
  const formRole = showViewForm ? viewingRole : editingRole;
  const formRoleId = showViewForm ? viewingRoleId : editingRoleId;

  const listHref = {
    pathname: "/t/[team]/settings/custom-roles",
    query: { team: team.slug },
  } as const;

  const goToList = () => {
    void router.push(listHref, undefined, { shallow: true });
  };

  const goToNew = (templateId?: string) => {
    void router.push(
      {
        pathname: "/t/[team]/settings/custom-roles",
        query: {
          team: team.slug,
          new: "1",
          ...(templateId ? { template: templateId } : {}),
        },
      },
      undefined,
      { shallow: true },
    );
  };

  const goToEdit = (roleId: number) => {
    void router.push(
      {
        pathname: "/t/[team]/settings/custom-roles",
        query: { team: team.slug, role: roleId.toString() },
      },
      undefined,
      { shallow: true },
    );
  };

  const goToView = (roleId: number) => {
    void router.push(
      {
        pathname: "/t/[team]/settings/custom-roles",
        query: { team: team.slug, view: roleId.toString() },
      },
      undefined,
      { shallow: true },
    );
  };

  const currentCrumb = isNewRole
    ? "New"
    : showViewForm
      ? (viewingRole?.name ?? "View")
      : (editingRole?.name ?? (editingRoleId ? "Edit" : undefined));

  return (
    <div className="-mx-6 flex min-h-0 flex-1 flex-col">
      <div className="sticky top-0 z-10 -mt-6 flex items-center gap-2 bg-background-primary p-6">
        {showForm ? (
          <Link href={listHref}>
            <h2>Custom Roles</h2>
          </Link>
        ) : (
          <h2>Custom Roles</h2>
        )}
        {showForm && currentCrumb && (
          <>
            <span className="text-content-secondary" role="separator">
              /
            </span>
            <h2 className="text-content-secondary">{currentCrumb}</h2>
          </>
        )}
      </div>
      <div className="relative flex min-h-0 flex-1 overflow-x-hidden">
        <div
          className={cn(
            "flex min-h-0 w-full flex-1 gap-6 transition-transform duration-500 motion-reduce:transition-none",
            showForm ? "-translate-x-[calc(100%+1.5rem)]" : "translate-x-0",
          )}
        >
          <div
            className={cn(
              "flex w-full shrink-0 flex-col gap-4 px-6",
              showForm ? "pointer-events-none select-none" : "",
            )}
            // @ts-expect-error https://github.com/facebook/react/issues/17157
            inert={showForm ? "inert" : undefined}
          >
            {entitlementsLoaded && !customRolesEnabled && (
              <Callout variant="upsell">
                Custom roles are available on the Business plan. Upgrade your
                team to create and manage custom roles.
              </Callout>
            )}
            <Sheet>
              <div className="mb-4 flex items-center justify-between gap-4">
                <p className="text-sm text-content-primary">
                  Custom roles let you define fine-grained permissions for your
                  team members.{" "}
                  <Link
                    href="https://docs.convex.dev/team-management/custom-roles"
                    target="_blank"
                  >
                    Learn more about custom roles
                  </Link>
                  .
                </p>
                <Menu
                  placement="bottom-end"
                  buttonProps={{
                    icon: <PlusIcon />,
                    disabled: !canManage,
                    tip: disabledReason,
                    children: "Create Role",
                  }}
                >
                  {[
                    <div
                      key="__header__"
                      className="mx-3 pb-1 text-xs text-content-secondary select-none"
                    >
                      Role Template
                    </div>,
                    ...CUSTOM_ROLE_TEMPLATES.map((t) => (
                      <MenuItem
                        key={t.id}
                        action={() => goToNew(t.id)}
                        tip={t.description}
                        tipSide="left"
                      >
                        {t.label}
                      </MenuItem>
                    )),
                    <hr key="__divider__" className="mx-3 my-1 border-t" />,
                    <MenuItem key="__blank__" action={() => goToNew()}>
                      Start without a template
                    </MenuItem>,
                  ]}
                </Menu>
              </div>
              {roles === undefined ? (
                <Loading fullHeight={false} className="h-14 w-full" />
              ) : (
                <div className="flex w-full flex-col divide-y">
                  {roles.length > 0 ? (
                    roles.map((role) => (
                      <CustomRoleListItem
                        key={role.id}
                        role={role}
                        teamId={team.id}
                        onView={() => goToView(role.id)}
                        onEdit={() => goToEdit(role.id)}
                        disabled={!canManage}
                        disabledReason={disabledReason}
                      />
                    ))
                  ) : (
                    <div className="my-6 flex w-full justify-center text-content-secondary">
                      No custom roles have been created yet.
                    </div>
                  )}
                </div>
              )}
            </Sheet>
          </div>
          <div
            className={cn(
              "flex w-full shrink-0 flex-col gap-4 px-6",
              !showForm ? "pointer-events-none select-none" : "",
            )}
            // @ts-expect-error https://github.com/facebook/react/issues/17157
            inert={!showForm ? "inert" : undefined}
          >
            {showForm &&
              (formRoleId !== undefined && customRolesData === undefined ? (
                <Loading fullHeight={false} className="h-48 w-full" />
              ) : (
                // Stable key: the form is unmounted whenever `showForm` flips
                // to false (Cancel / breadcrumb), so a fresh mount picks up
                // the right `existingRole`. Keying by `editingRoleId` would
                // remount on the create→edit URL flip and wipe the editor.
                <CustomRoleForm
                  key="custom-role-form"
                  teamId={team.id}
                  teamSlug={team.slug}
                  existingRole={formRole}
                  templateId={newRoleTemplateId}
                  readOnly={showViewForm}
                  onClose={goToList}
                  onSaved={goToEdit}
                />
              ))}
          </div>
        </div>
      </div>
    </div>
  );
}
