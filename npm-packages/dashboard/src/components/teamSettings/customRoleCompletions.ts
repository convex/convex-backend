import type { Monaco } from "@monaco-editor/react";
import type { IDisposable, IRange, editor, languages } from "monaco-editor";
import { RoleStatementAction } from "@convex-dev/platform/managementApi";
import {
  getLocation,
  parseTree,
  findNodeAtLocation,
  type Node,
} from "jsonc-parser";
import { ACTIONS_BY_CATEGORY, ActionCategory } from "./customRoleActions";

type ITextModel = editor.ITextModel;
type IPosition = { lineNumber: number; column: number };
type CompletionItem = languages.CompletionItem;

// Resource template list shown when typing a `"resource"` value. Each entry
// is inserted as literal text — the user can edit selectors after acceptance
// using normal IntelliSense rather than tabbing through snippet placeholders.
type ResourceTemplate = {
  label: string;
  insertText: string;
  detail: string;
  documentation: string;
  category: ActionCategory;
};
const RESOURCE_TEMPLATES: ResourceTemplate[] = [
  {
    label: "team:*",
    insertText: "team:*",
    detail: "All team-level actions",
    documentation: "Matches the team itself for team:* actions.",
    category: "team",
  },
  {
    label: "team:*:token:*",
    insertText: "team:*:token:*",
    detail: "Team access tokens",
    documentation:
      "Team-scoped tokens. Narrow with `creator=self` or `creator=<memberId>`.",
    category: "teamToken",
  },
  {
    label: "project:*",
    insertText: "project:*",
    detail: "All projects (or filter by id/slug)",
    documentation:
      "Matches projects. Use `*`, `id=<projectId>`, or `slug=<project-slug>`.",
    category: "project",
  },
  {
    label: "project:id=",
    insertText: "project:id=",
    detail: "A specific project by id",
    documentation: "Matches one project by numeric id.",
    category: "project",
  },
  {
    label: "project:slug=",
    insertText: "project:slug=",
    detail: "A specific project by slug",
    documentation: "Matches one project by URL slug.",
    category: "project",
  },
  {
    label: "project:*:deployment:*",
    insertText: "project:*:deployment:*",
    detail: "Deployments in matching projects",
    documentation:
      "Second selector supports `*`, `id=<deploymentId>`, `type=<prod|dev|preview|custom>`, or `creator=self|<memberId>`.",
    category: "deployment",
  },
  {
    label: "project:*:deployment:type=prod",
    insertText: "project:*:deployment:type=prod",
    detail: "Production deployments",
    documentation:
      'Common with `effect: "deny"` to wall off production deployments.',
    category: "deployment",
  },
  {
    label: "project:*:deployment:type=dev",
    insertText: "project:*:deployment:type=dev",
    detail: "Development deployments",
    documentation: "Personal dev deployments across matching projects.",
    category: "deployment",
  },
  {
    label: "project:*:deployment:type=preview",
    insertText: "project:*:deployment:type=preview",
    detail: "Preview deployments",
    documentation: "Preview deployments (PR-style ephemeral deployments).",
    category: "deployment",
  },
  {
    label: "project:*:deployment:type=custom",
    insertText: "project:*:deployment:type=custom",
    detail: "Custom deployments",
    documentation: "Custom deployments across matching projects.",
    category: "deployment",
  },
  {
    label: "project:*:deployment:creator=self",
    insertText: "project:*:deployment:creator=self",
    detail: "Deployments created by the acting member",
    documentation:
      "`self` resolves to the acting member at evaluation time. Use `creator=<memberId>` to target a specific member.",
    category: "deployment",
  },
  {
    label: "project:*:token:*",
    insertText: "project:*:token:*",
    detail: "Project access tokens",
    documentation: "Project-scoped tokens.",
    category: "projectToken",
  },
  {
    label: "project:*:deployment:*:token:*",
    insertText: "project:*:deployment:*:token:*",
    detail: "Deployment access tokens",
    documentation: "Deployment-scoped tokens.",
    category: "deploymentToken",
  },
  {
    label: "project:*:defaultEnvironmentVariable:*",
    insertText: "project:*:defaultEnvironmentVariable:*",
    detail: "Project-default environment variables",
    documentation: "Default env vars cloned into new deployments.",
    category: "defaultEnvironmentVariable",
  },
  {
    label: "member:*",
    insertText: "member:*",
    detail: "All team members",
    documentation:
      "Matches all team members. The `member` resource only supports `*` — individual members can't be targeted by id.",
    category: "member",
  },
  {
    label: "customRole:*",
    insertText: "customRole:*",
    detail: "Custom role definitions",
    documentation: "Only `customRole:view` is delegable here.",
    category: "customRole",
  },
  {
    label: "billing:*",
    insertText: "billing:*",
    detail: "Billing",
    documentation: "All billing actions are scoped to this single resource.",
    category: "billing",
  },
  {
    label: "oauthApplication:*",
    insertText: "oauthApplication:*",
    detail: "OAuth applications",
    documentation: "Team-owned OAuth client registrations.",
    category: "oauthApplication",
  },
  {
    label: "sso:*",
    insertText: "sso:*",
    detail: "Single sign-on",
    documentation: "Team SSO configuration.",
    category: "sso",
  },
  {
    label: "directorySync:*",
    insertText: "directorySync:*",
    detail: "Directory Sync",
    documentation: "Team Directory Sync (SCIM) configuration.",
    category: "directorySync",
  },
  {
    label: "integration:*",
    insertText: "integration:*",
    detail: "Team integrations",
    documentation: "Team-level integrations (Datadog, Sentry, etc.).",
    category: "integration",
  },
];

const RESOURCE_PATTERNS: Array<{ regex: RegExp; category: ActionCategory }> = [
  { regex: /^team:[^:]+$/, category: "team" },
  { regex: /^team:[^:]+:token:[^:]+$/, category: "teamToken" },
  { regex: /^project:[^:]+$/, category: "project" },
  { regex: /^project:[^:]+:token:[^:]+$/, category: "projectToken" },
  { regex: /^project:[^:]+:deployment:[^:]+$/, category: "deployment" },
  {
    regex: /^project:[^:]+:deployment:[^:]+:token:[^:]+$/,
    category: "deploymentToken",
  },
  {
    regex: /^project:[^:]+:defaultEnvironmentVariable:[^:]+$/,
    category: "defaultEnvironmentVariable",
  },
  { regex: /^member:[^:]+$/, category: "member" },
  { regex: /^customRole:[^:]+$/, category: "customRole" },
  { regex: /^billing:[^:]+$/, category: "billing" },
  { regex: /^oauthApplication:[^:]+$/, category: "oauthApplication" },
  { regex: /^sso:[^:]+$/, category: "sso" },
  { regex: /^directorySync:[^:]+$/, category: "directorySync" },
  { regex: /^integration:[^:]+$/, category: "integration" },
];

function categoryForResource(resource: string): ActionCategory | null {
  for (const { regex, category } of RESOURCE_PATTERNS) {
    if (regex.test(resource)) return category;
  }
  return null;
}

// The editor buffer may be invalid JSON mid-edit, so we use `jsonc-parser`
// (the same tolerant parser that powers Monaco's JSON support). It hands us
// the path to the cursor and a forgiving syntax tree we can walk for siblings.

type Context =
  // Cursor is inside a "..." string token that is the value of `"effect"`.
  | { kind: "effect"; replace: { start: number; end: number } }
  // Cursor is inside a "..." string token that is the value of `"resource"`.
  | { kind: "resource"; replace: { start: number; end: number } }
  // Cursor is inside a "..." string token inside an `"actions"` array.
  | {
      kind: "actions-item";
      replace: { start: number; end: number };
      siblingResource: string | null;
    }
  // Cursor is inside the string value of `"actions"` (i.e. `"actions": "|"`).
  // Only `"*"` is valid here — array form is handled separately.
  | { kind: "actions-value"; replace: { start: number; end: number } }
  // Cursor sits between tokens at a position where a string value would go.
  | { kind: "effect-empty" | "resource-empty"; insertAt: number }
  // Cursor is at the value position of `"actions":` before any `[` or `"`.
  // Both `"*"` and `[]` are valid starting points.
  | { kind: "actions-value-empty"; insertAt: number }
  | {
      kind: "actions-item-empty";
      insertAt: number;
      siblingResource: string | null;
    }
  | { kind: "none" };

function getStringContentEnd(node: Node, text: string): number {
  const last = node.offset + node.length - 1;
  return text[last] === '"' ? last : node.offset + node.length;
}

function getSiblingResource(
  root: Node | undefined,
  statementIndex: number,
): string | null {
  if (!root) return null;
  const node = findNodeAtLocation(root, [statementIndex, "resource"]);
  return node?.type === "string" && typeof node.value === "string"
    ? node.value
    : null;
}

function analyzeContext(text: string, offset: number): Context {
  const loc = getLocation(text, offset);
  if (loc.isAtPropertyKey) return { kind: "none" };

  const root = parseTree(text);
  const existing = root ? findNodeAtLocation(root, loc.path) : undefined;

  // Cursor is inside an existing string value at this path if its offset
  // falls within the node's range (past the opening quote).
  const insideString =
    existing?.type === "string" &&
    offset > existing.offset &&
    offset <= existing.offset + existing.length;

  const replace = insideString
    ? { start: existing!.offset + 1, end: getStringContentEnd(existing!, text) }
    : null;

  // An "empty" slot is one with no value parsed yet at this path. If a value
  // already exists and the cursor isn't inside it, we don't offer completions.
  const slotEmpty = existing === undefined;

  const path = loc.path;

  // Statement-level path: [statementIndex, key].
  if (path.length === 2 && typeof path[0] === "number") {
    const key = path[1];
    if (key === "effect") {
      if (insideString) return { kind: "effect", replace: replace! };
      if (slotEmpty) return { kind: "effect-empty", insertAt: offset };
    } else if (key === "resource") {
      if (insideString) return { kind: "resource", replace: replace! };
      if (slotEmpty) return { kind: "resource-empty", insertAt: offset };
    } else if (key === "actions") {
      if (insideString) return { kind: "actions-value", replace: replace! };
      if (slotEmpty) return { kind: "actions-value-empty", insertAt: offset };
    }
  }

  // Actions array item: [statementIndex, "actions", itemIndex].
  if (
    path.length === 3 &&
    typeof path[0] === "number" &&
    path[1] === "actions" &&
    typeof path[2] === "number"
  ) {
    const siblingResource = getSiblingResource(root, path[0]);
    if (insideString)
      return { kind: "actions-item", replace: replace!, siblingResource };
    if (slotEmpty)
      return {
        kind: "actions-item-empty",
        insertAt: offset,
        siblingResource,
      };
  }

  return { kind: "none" };
}

function makeRange(
  model: ITextModel,
  startOffset: number,
  endOffset: number,
): IRange {
  const start = model.getPositionAt(startOffset);
  const end = model.getPositionAt(endOffset);
  return {
    startLineNumber: start.lineNumber,
    startColumn: start.column,
    endLineNumber: end.lineNumber,
    endColumn: end.column,
  };
}

const EFFECT_SUGGESTIONS: Array<{ value: string; doc: string }> = [
  {
    value: "allow",
    doc: "Grant the listed actions on the matching resource.",
  },
  {
    value: "deny",
    doc: "Block the listed actions even if another statement allows them. Deny wins.",
  },
];

function buildEffectSuggestions(
  monaco: Monaco,
  range: IRange,
  insideString: boolean,
): CompletionItem[] {
  return EFFECT_SUGGESTIONS.map(({ value, doc }) => ({
    label: value,
    kind: monaco.languages.CompletionItemKind.EnumMember,
    insertText: insideString ? value : `"${value}"`,
    range,
    documentation: { value: doc },
    sortText: value,
  }));
}

function buildResourceSuggestions(
  monaco: Monaco,
  range: IRange,
  insideString: boolean,
): CompletionItem[] {
  return RESOURCE_TEMPLATES.map((tpl, i) => ({
    label: { label: tpl.label, description: tpl.detail },
    kind: monaco.languages.CompletionItemKind.Value,
    insertText: insideString ? tpl.insertText : `"${tpl.insertText}"`,
    range,
    detail: tpl.detail,
    documentation: { value: tpl.documentation },
    sortText: String(i).padStart(3, "0"),
  }));
}

function buildActionSuggestions(
  monaco: Monaco,
  range: IRange,
  insideString: boolean,
  siblingResource: string | null,
): CompletionItem[] {
  const category = siblingResource
    ? categoryForResource(siblingResource)
    : null;
  let actions: RoleStatementAction[];
  if (category) {
    actions = [...ACTIONS_BY_CATEGORY[category]];
  } else {
    // Fallback: union of all actions across categories.
    const seen = new Set<RoleStatementAction>();
    for (const list of Object.values(ACTIONS_BY_CATEGORY)) {
      for (const a of list) seen.add(a);
    }
    actions = Array.from(seen).sort();
  }
  return actions.map((action, i) => ({
    label: action,
    kind: monaco.languages.CompletionItemKind.EnumMember,
    insertText: insideString ? action : `"${action}"`,
    range,
    sortText: String(i).padStart(3, "0"),
  }));
}

const WILDCARD_DOC =
  'Wildcard. Grants every action valid for the statement\'s `"resource"`. Cannot be combined with other actions.';

function buildActionsValueSuggestions(
  monaco: Monaco,
  range: IRange,
  insideString: boolean,
): CompletionItem[] {
  if (insideString) {
    return [
      {
        label: { label: "*", description: "All actions for this resource" },
        kind: monaco.languages.CompletionItemKind.Keyword,
        insertText: "*",
        range,
        documentation: { value: WILDCARD_DOC },
        sortText: "0",
      },
    ];
  }
  return [
    {
      label: { label: '"*"', description: "All actions for this resource" },
      kind: monaco.languages.CompletionItemKind.Keyword,
      insertText: '"*"',
      range,
      documentation: { value: WILDCARD_DOC },
      sortText: "0",
    },
    {
      label: { label: "[…]", description: "Specific actions" },
      kind: monaco.languages.CompletionItemKind.Snippet,
      insertText: '[\n  "$0"\n]',
      insertTextRules:
        monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
      range,
      documentation: {
        value:
          "Pick specific actions. The list will autocomplete based on the statement's resource.",
      },
      sortText: "1",
    },
  ];
}

export function registerCustomRoleCompletions(
  monaco: Monaco,
  modelUri: string,
): IDisposable {
  return monaco.languages.registerCompletionItemProvider("json", {
    triggerCharacters: ['"', ":", ",", "[", " "],
    provideCompletionItems(
      model: ITextModel,
      position: IPosition,
    ): { suggestions: CompletionItem[] } {
      if (model.uri.toString() !== modelUri) return { suggestions: [] };
      const text = model.getValue();
      const offset = model.getOffsetAt(position);
      const ctx = analyzeContext(text, offset);

      switch (ctx.kind) {
        case "effect": {
          const range = makeRange(model, ctx.replace.start, ctx.replace.end);
          return { suggestions: buildEffectSuggestions(monaco, range, true) };
        }
        case "effect-empty": {
          const range = makeRange(model, ctx.insertAt, ctx.insertAt);
          return { suggestions: buildEffectSuggestions(monaco, range, false) };
        }
        case "resource": {
          const range = makeRange(model, ctx.replace.start, ctx.replace.end);
          return { suggestions: buildResourceSuggestions(monaco, range, true) };
        }
        case "resource-empty": {
          const range = makeRange(model, ctx.insertAt, ctx.insertAt);
          return {
            suggestions: buildResourceSuggestions(monaco, range, false),
          };
        }
        case "actions-value": {
          const range = makeRange(model, ctx.replace.start, ctx.replace.end);
          return {
            suggestions: buildActionsValueSuggestions(monaco, range, true),
          };
        }
        case "actions-value-empty": {
          const range = makeRange(model, ctx.insertAt, ctx.insertAt);
          return {
            suggestions: buildActionsValueSuggestions(monaco, range, false),
          };
        }
        case "actions-item": {
          const range = makeRange(model, ctx.replace.start, ctx.replace.end);
          return {
            suggestions: buildActionSuggestions(
              monaco,
              range,
              true,
              ctx.siblingResource,
            ),
          };
        }
        case "actions-item-empty": {
          const range = makeRange(model, ctx.insertAt, ctx.insertAt);
          return {
            suggestions: buildActionSuggestions(
              monaco,
              range,
              false,
              ctx.siblingResource,
            ),
          };
        }
        default:
          return { suggestions: [] };
      }
    },
  });
}
