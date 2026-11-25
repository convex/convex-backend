#!/usr/bin/env node
// Overwrites the `.md` copy of each HTTP API endpoint page in `build/` with
// Markdown generated from its OpenAPI operation.
//
// The llms-txt plugin derives each page's `.md` from the built HTML, but
// docusaurus-theme-openapi-docs renders parameters, request bodies and
// response schemas only in the browser, so the HTML-derived Markdown has none
// of them. The "Copy as Markdown" button and the `Accept: text/markdown` edge
// function both serve these files.
//
// Runs after `docusaurus build`, reading the specs written by
// prepare-openapi-specs.js. Endpoint pages are matched to operations through
// the `id` front matter and `<MethodEndpoint>` of the generated `.api.mdx`
// files, so the slugs always agree with the plugin's.
const fs = require("fs");
const path = require("path");

const HTTP_METHODS = [
  "get",
  "put",
  "post",
  "delete",
  "options",
  "head",
  "patch",
  "trace",
];

// Keep in sync with the `docusaurus-plugin-openapi-docs` config in
// docusaurus.config.ts.
const APIS = [
  {
    spec: ".openapi-filtered/management-openapi.json",
    docsDir: "docs/management-api",
  },
  {
    spec: ".openapi-filtered/public-deployment-openapi.json",
    docsDir: "docs/public-deployment-api",
  },
  {
    spec: ".openapi-filtered/deployment-openapi.json",
    docsDir: "docs/deployment-api",
  },
];

const AGENT_NOTE =
  "> For AI agents: see [llms.txt](/llms.txt) for the complete documentation index. " +
  "Markdown versions are available by adding .md to a page URL or requesting Accept: text/markdown.";

const docsRoot = path.resolve(__dirname, "..");
const buildDir = path.join(docsRoot, "build");

function resolvePointer(spec, ref) {
  if (!ref.startsWith("#/")) {
    throw new Error(`Unsupported external $ref: ${ref}`);
  }
  return ref
    .slice(2)
    .split("/")
    .map((part) => part.replace(/~1/g, "/").replace(/~0/g, "~"))
    .reduce((node, part) => node?.[part], spec);
}

// Inlines every local `$ref`. A schema that refers back to one of its own
// ancestors keeps that `$ref` (the name stays readable), so recursive types
// terminate.
function inlineRefs(spec, value, ancestors = []) {
  if (Array.isArray(value)) {
    return value.map((item) => inlineRefs(spec, item, ancestors));
  }
  if (value === null || typeof value !== "object") {
    return value;
  }
  if (typeof value.$ref === "string") {
    if (ancestors.includes(value.$ref)) {
      return { $ref: value.$ref };
    }
    const { $ref, ...siblings } = value;
    const target = resolvePointer(spec, $ref);
    if (target === undefined) {
      throw new Error(`Unresolvable $ref: ${$ref}`);
    }
    return inlineRefs(spec, { ...target, ...siblings }, [...ancestors, $ref]);
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, child]) => [
      key,
      inlineRefs(spec, child, ancestors),
    ]),
  );
}

// Rewrites Docusaurus admonitions (`:::info[Title] ... :::`, as added by
// prepare-openapi-specs.js) as blockquotes, which plain Markdown renders.
function convertAdmonitions(markdown) {
  return markdown.replace(
    /^:::(\w+)(?:\[([^\]]*)\])?\n([\s\S]*?)\n:::$/gm,
    (_, kind, title, body) => {
      const heading = `**${title || kind[0].toUpperCase() + kind.slice(1)}**`;
      const lines = [heading, "", ...body.trim().split("\n")];
      return lines.map((line) => (line ? `> ${line}` : ">")).join("\n");
    },
  );
}

function escapeTableCell(text) {
  return String(text).replace(/\|/g, "\\|").replace(/\n+/g, " ");
}

function schemaType(schema) {
  if (!schema) return "";
  if (schema.type === "array") return `${schemaType(schema.items)}[]`;
  if (Array.isArray(schema.type)) return schema.type.join(" \\| ");
  if (schema.type)
    return schema.format ? `${schema.type} (${schema.format})` : schema.type;
  if (schema.enum) return "enum";
  if (schema.oneOf || schema.anyOf) return "one of";
  return "";
}

function jsonBlock(value) {
  return "```json\n" + JSON.stringify(value, null, 2) + "\n```";
}

function renderParameters(parameters) {
  if (parameters.length === 0) return [];
  const lines = [
    "### Parameters",
    "",
    "| Name | In | Type | Required | Description |",
    "| --- | --- | --- | --- | --- |",
  ];
  for (const param of parameters) {
    const type = schemaType(param.schema);
    const enumValues = param.schema?.enum
      ? ` One of: ${param.schema.enum.map((v) => `\`${v}\``).join(", ")}.`
      : "";
    lines.push(
      `| \`${param.name}\` | ${param.in} | ${escapeTableCell(type)} | ${
        param.required ? "yes" : "no"
      } | ${escapeTableCell((param.description ?? "") + enumValues)} |`,
    );
  }
  lines.push("");
  return lines;
}

function renderContent(content) {
  const lines = [];
  for (const [mediaType, media] of Object.entries(content ?? {})) {
    lines.push(`Content type: \`${mediaType}\``, "");
    if (media.schema) {
      lines.push(jsonBlock(media.schema), "");
    }
    if (media.example !== undefined) {
      lines.push("Example:", "", jsonBlock(media.example), "");
    }
  }
  return lines;
}

// The public deployment API spec has a relative server URL (`/api`), which is
// relative to the deployment URL. Spelled out as a `{deployment-url}`
// variable, it matches the deployment API spec's server.
function absoluteServer(server) {
  if (!server?.url?.startsWith("/")) return server;
  return {
    ...server,
    url: `{deployment-url}${server.url}`,
    variables: {
      "deployment-url": {
        default: "https://happy-animal-123.convex.cloud",
        description: "Your deployment URL",
      },
      ...server.variables,
    },
  };
}

function renderOperation({ spec, method, pathStr, pathItem, op, title }) {
  const resolved = inlineRefs(spec, {
    parameters: [...(pathItem.parameters ?? []), ...(op.parameters ?? [])],
    requestBody: op.requestBody,
    responses: op.responses,
  });
  const server = absoluteServer(
    (op.servers ?? pathItem.servers ?? spec.servers)?.[0],
  );
  const lines = [AGENT_NOTE, "", `# ${title}`, ""];

  lines.push(
    "```",
    `${method.toUpperCase()} ${(server?.url ?? "").replace(/\/$/, "")}${pathStr}`,
    "```",
    "",
  );
  for (const [name, variable] of Object.entries(server?.variables ?? {})) {
    const example = variable.default
      ? ` (for example \`${variable.default}\`)`
      : "";
    lines.push(`\`{${name}}\`: ${variable.description ?? ""}${example}.`, "");
  }
  if (op.description ?? op.summary) {
    lines.push(convertAdmonitions((op.description ?? op.summary).trim()), "");
  }

  const security = op.security ?? spec.security;
  if (security?.length) {
    const schemes = security.flatMap(Object.keys);
    const descriptions = schemes.map((name) => {
      const scheme = spec.components?.securitySchemes?.[name];
      const detail =
        scheme?.type === "http"
          ? `HTTP ${scheme.scheme} (\`Authorization: ${
              scheme.scheme === "bearer" ? "Bearer" : scheme.scheme
            } <token>\`)`
          : scheme?.type === "apiKey"
            ? `API key in the \`${scheme.name}\` ${scheme.in}`
            : (scheme?.type ?? "");
      return `- \`${name}\`${detail ? `: ${detail}` : ""}${
        scheme?.description ? `. ${scheme.description.trim()}` : ""
      }`;
    });
    // Each entry of an OpenAPI `security` array is an alternative.
    lines.push(
      "## Authentication",
      "",
      descriptions.length > 1 ? "Any one of:" : "",
      "",
      ...descriptions,
      "",
    );
  }

  lines.push("## Request", "");
  lines.push(...renderParameters(resolved.parameters));
  if (resolved.requestBody) {
    lines.push(
      `### Body${resolved.requestBody.required ? " (required)" : ""}`,
      "",
    );
    if (resolved.requestBody.description) {
      lines.push(resolved.requestBody.description.trim(), "");
    }
    lines.push(...renderContent(resolved.requestBody.content));
  }
  if (resolved.parameters.length === 0 && !resolved.requestBody) {
    lines.push("This endpoint takes no parameters or body.", "");
  }

  lines.push("## Responses", "");
  for (const [status, response] of Object.entries(resolved.responses ?? {})) {
    lines.push(`### ${status}`, "");
    if (response.description) {
      lines.push(response.description.trim(), "");
    }
    lines.push(...renderContent(response.content));
  }

  return (
    lines
      .join("\n")
      .replace(/\n{3,}/g, "\n\n")
      .trimEnd() + "\n"
  );
}

// Maps `METHOD /path` to the page id of each generated endpoint page.
function readEndpointPages(docsDir) {
  const pages = new Map();
  for (const file of fs.readdirSync(docsDir)) {
    if (!file.endsWith(".api.mdx")) continue;
    const source = fs.readFileSync(path.join(docsDir, file), "utf8");
    const id = source.match(/^id: (.+)$/m)?.[1]?.trim();
    const title = source.match(/^title: "?(.*?)"?$/m)?.[1];
    const endpoint = source.match(
      /<MethodEndpoint\s+method=\{"(\w+)"\}\s+path=\{"([^"]+)"\}/,
    );
    if (!id || !endpoint) {
      throw new Error(`Can't find the id or endpoint of ${file}`);
    }
    pages.set(`${endpoint[1].toLowerCase()} ${endpoint[2]}`, { id, title });
  }
  return pages;
}

function main() {
  let written = 0;
  for (const api of APIS) {
    const spec = JSON.parse(
      fs.readFileSync(path.join(docsRoot, api.spec), "utf8"),
    );
    const pages = readEndpointPages(path.join(docsRoot, api.docsDir));
    const outDir = path.join(buildDir, path.relative("docs", api.docsDir));

    for (const [pathStr, pathItem] of Object.entries(spec.paths ?? {})) {
      for (const method of HTTP_METHODS) {
        const op = pathItem[method];
        if (!op) continue;
        // The theme shows `{param}` path segments as `:param`.
        const displayPath = pathStr.replace(/\{([^}]+)\}/g, ":$1");
        const page =
          pages.get(`${method} ${displayPath}`) ??
          pages.get(`${method} ${pathStr}`);
        if (!page) {
          throw new Error(
            `No generated page for ${method.toUpperCase()} ${pathStr} in ${api.docsDir}`,
          );
        }
        const target = path.join(outDir, `${page.id}.md`);
        if (!fs.existsSync(target)) {
          throw new Error(
            `Expected the llms-txt plugin to have written ${target}`,
          );
        }
        fs.writeFileSync(
          target,
          renderOperation({
            spec,
            method,
            pathStr,
            pathItem,
            op,
            title: page.title ?? op.summary ?? op.operationId,
          }),
        );
        written++;
      }
    }
  }
  console.log(`Wrote ${written} HTTP API endpoint pages as Markdown.`);
}

main();
