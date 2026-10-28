import { ValidatorJSON, ObjectFieldType } from "convex/values";
import { Shape } from "shapes";
import { SchemaJson, prettier } from "@common/lib/format";

/**
 * A single top-level field of a table, summarized for display in the schema
 * visualizer. `references` lists the tables that this field points at via
 * `v.id("...")` (directly, or nested inside arrays/records/unions/objects).
 */
export type SchemaField = {
  name: string;
  // A compact, human-readable type label, e.g. `Id<users>`, `string`,
  // `Id<messages>[]` or `{ … }`.
  type: string;
  // The full TypeScript-style type with object bodies expanded (e.g.
  // `{ theme: "light" | "dark" }`), present only when the compact label hides
  // detail (i.e. it collapsed an object to `{ … }`). Used to reveal the
  // complete type when a field is expanded in the side panel.
  fullType?: string;
  optional: boolean;
  references: string[];
};

/**
 * An index defined on a table. `kind` distinguishes database indexes from
 * search and vector indexes. `fields` is the indexed field list (for search and
 * vector indexes, the search/vector field followed by any filter fields).
 */
export type SchemaIndex = {
  name: string;
  kind: "database" | "search" | "vector";
  fields: string[];
};

export type SchemaUnionVariant = {
  label: string;
  fields: SchemaField[];
};

export type SchemaUnion = {
  discriminator?: string;
  variants: SchemaUnionVariant[];
};

export type SchemaNode = {
  table: string;
  fields: SchemaField[];
  union?: SchemaUnion;
  // Indexes defined on the table. Only available from a saved schema (shapes
  // can't infer indexes), so this is empty for shape-derived graphs.
  indexes: SchemaIndex[];
  // True when the table exists in the deployment's data but isn't declared in
  // the saved schema (its fields are inferred from shapes). Always false for
  // pure shape-derived graphs — without a schema, there's nothing to be missing
  // from.
  notInSchema: boolean;
};

export type SchemaEdge = {
  id: string;
  source: string;
  target: string;
  field: string;
  optional: boolean;
};

export type SchemaGraph = {
  nodes: SchemaNode[];
  edges: SchemaEdge[];
};

// --- Validator (saved schema) helpers ---------------------------------------

function collectReferencesFromValidator(
  validator: ValidatorJSON,
  acc: Set<string>,
): void {
  switch (validator.type) {
    case "id":
      acc.add(validator.tableName);
      break;
    case "array":
      collectReferencesFromValidator(validator.value, acc);
      break;
    case "record":
      collectReferencesFromValidator(validator.values.fieldType, acc);
      break;
    case "object":
      Object.values(validator.value).forEach((field) =>
        collectReferencesFromValidator(field.fieldType, acc),
      );
      break;
    case "union":
      validator.value.forEach((v) => collectReferencesFromValidator(v, acc));
      break;
    default:
      break;
  }
}

// A TypeScript-style type label for a validator. By default objects collapse to
// `{ … }` to stay compact inside a graph node; pass `expand` to render the full
// object body (e.g. `{ name: string; theme: "light" | "dark" }`).
function validatorToLabel(validator: ValidatorJSON, expand = false): string {
  switch (validator.type) {
    case "null":
      return "null";
    case "number":
      return "number";
    case "bigint":
      return "bigint";
    case "commitTs":
      return "commitTs";
    case "boolean":
      return "boolean";
    case "string":
      return "string";
    case "bytes":
      return "bytes";
    case "any":
      return "any";
    case "literal":
      return typeof validator.value === "string"
        ? `"${validator.value}"`
        : String(validator.value);
    case "id":
      return `Id<"${validator.tableName}">`;
    case "array":
      return `${validatorToLabel(validator.value, expand)}[]`;
    case "record":
      return `Record<${validatorToLabel(
        validator.keys,
        expand,
      )}, ${validatorToLabel(validator.values.fieldType, expand)}>`;
    case "object": {
      if (!expand) {
        return "{ … }";
      }
      const entries = Object.entries(validator.value);
      if (entries.length === 0) {
        return "{}";
      }
      return `{ ${entries
        .map(
          ([name, field]) =>
            `${name}${field.optional ? "?" : ""}: ${validatorToLabel(
              field.fieldType,
              true,
            )}`,
        )
        .join("; ")} }`;
    }
    case "union":
      return validator.value
        .map((v) => validatorToLabel(v, expand))
        .join(" | ");
    default:
      return "any";
  }
}

// Pretty-print a TypeScript type expression by formatting it as a type alias
// (so the parser accepts the bare type) and stripping the wrapper. Falls back
// to the unformatted type if formatting fails.
function prettifyType(type: string): string {
  try {
    return prettier(`type _ = ${type}`, 40)
      .replace(/^type _ =\s*/, "")
      .replace(/;\s*$/, "")
      .trim();
  } catch {
    return type;
  }
}

function fieldsFromObjectValidator(
  fields: Record<string, ObjectFieldType>,
): SchemaField[] {
  return Object.entries(fields).map(([name, field]) => {
    const references = new Set<string>();
    collectReferencesFromValidator(field.fieldType, references);
    const type = validatorToLabel(field.fieldType);
    return {
      name,
      type,
      // Only carry the full form when the compact label collapsed something.
      fullType: type.includes("…")
        ? prettifyType(validatorToLabel(field.fieldType, true))
        : undefined,
      optional: field.optional,
      references: Array.from(references),
    };
  });
}

function uniqueInOrder(values: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  values.forEach((v) => {
    if (!seen.has(v)) {
      seen.add(v);
      out.push(v);
    }
  });
  return out;
}

function mergeVariantFields(variantFields: SchemaField[][]): SchemaField[] {
  const byName = new Map<string, SchemaField[]>();
  const order: string[] = [];
  variantFields.forEach((fields) => {
    fields.forEach((field) => {
      const existing = byName.get(field.name);
      if (existing) {
        existing.push(field);
      } else {
        byName.set(field.name, [field]);
        order.push(field.name);
      }
    });
  });
  return order.map((name) => {
    const occurrences = byName.get(name)!;
    const type = uniqueInOrder(occurrences.map((f) => f.type)).join(" | ");
    const fullType = uniqueInOrder(
      occurrences.map((f) => f.fullType ?? f.type),
    ).join(" | ");
    return {
      name,
      type,
      fullType: fullType !== type ? prettifyType(fullType) : undefined,
      optional:
        occurrences.length < variantFields.length ||
        occurrences.some((f) => f.optional),
      references: Array.from(new Set(occurrences.flatMap((f) => f.references))),
    };
  });
}

function literalToLabel(value: unknown): string {
  return typeof value === "string" ? value : String(value);
}

type ObjectValidator = Extract<ValidatorJSON, { type: "object" }>;

const DISCRIMINATOR_PREFERENCE = ["type", "kind", "variant", "tag", "_type"];

function detectDiscriminator(members: ObjectValidator[]): string | undefined {
  if (members.length < 2) {
    return undefined;
  }
  const candidates = Object.keys(members[0].value).filter((name) => {
    const seen = new Set<string>();
    for (const member of members) {
      const field = member.value[name];
      if (!field || field.optional || field.fieldType.type !== "literal") {
        return false;
      }
      const key = JSON.stringify(field.fieldType.value);
      if (seen.has(key)) {
        return false;
      }
      seen.add(key);
    }
    return true;
  });
  if (candidates.length === 0) {
    return undefined;
  }
  const rank = (name: string) => {
    const i = DISCRIMINATOR_PREFERENCE.indexOf(name);
    return i === -1 ? DISCRIMINATOR_PREFERENCE.length : i;
  };
  return candidates.sort((a, b) => rank(a) - rank(b))[0];
}

function orderDiscriminatorFirst(
  fields: SchemaField[],
  discriminator: string | undefined,
): SchemaField[] {
  if (!discriminator) {
    return fields;
  }
  const idx = fields.findIndex((f) => f.name === discriminator);
  if (idx <= 0) {
    return fields;
  }
  const reordered = [...fields];
  const [field] = reordered.splice(idx, 1);
  reordered.unshift(field);
  return reordered;
}

function unionFromValidator(validator: ValidatorJSON): SchemaUnion | undefined {
  if (validator.type !== "union") {
    return undefined;
  }
  const members = validator.value;
  if (
    members.length < 2 ||
    !members.every((m): m is ObjectValidator => m.type === "object")
  ) {
    return undefined;
  }
  const discriminator = detectDiscriminator(members);
  const variants: SchemaUnionVariant[] = members.map((member, i) => {
    let label = `Variant ${i + 1}`;
    if (discriminator) {
      const disc = member.value[discriminator]?.fieldType;
      if (disc?.type === "literal") {
        label = literalToLabel(disc.value);
      }
    }
    return {
      label,
      fields: orderDiscriminatorFirst(
        fieldsFromObjectValidator(member.value),
        discriminator,
      ),
    };
  });
  return { discriminator, variants };
}

function unionFromDocumentType(
  documentType: ValidatorJSON | null,
): SchemaUnion | undefined {
  return documentType ? unionFromValidator(documentType) : undefined;
}

function fieldsFromDocumentType(
  documentType: ValidatorJSON | null,
): SchemaField[] {
  if (!documentType) {
    return [];
  }
  if (documentType.type === "object") {
    return fieldsFromObjectValidator(documentType.value);
  }
  if (documentType.type === "union") {
    const members = documentType.value;
    const merged = mergeVariantFields(
      members.map((member) =>
        member.type === "object" ? fieldsFromObjectValidator(member.value) : [],
      ),
    );
    const discriminator = members.every(
      (m): m is ObjectValidator => m.type === "object",
    )
      ? detectDiscriminator(members)
      : undefined;
    return orderDiscriminatorFirst(merged, discriminator);
  }
  return [];
}

// --- Shape (inferred schema) helpers ----------------------------------------

function collectReferencesFromShape(shape: Shape, acc: Set<string>): void {
  switch (shape.type) {
    case "Id":
      acc.add(shape.tableName);
      break;
    case "Array":
      collectReferencesFromShape(shape.shape, acc);
      break;
    case "Record":
      collectReferencesFromShape(shape.valueShape.shape, acc);
      break;
    case "Object":
      shape.fields.forEach((f) => collectReferencesFromShape(f.shape, acc));
      break;
    case "Union":
      shape.shapes.forEach((s) => collectReferencesFromShape(s, acc));
      break;
    default:
      break;
  }
}

function shapeToLabel(shape: Shape, expand = false): string {
  switch (shape.type) {
    case "Id":
      return `Id<"${shape.tableName}">`;
    case "Array":
      return `${shapeToLabel(shape.shape, expand)}[]`;
    case "Record":
      return `Record<${shapeToLabel(shape.keyShape, expand)}, ${shapeToLabel(
        shape.valueShape.shape,
        expand,
      )}>`;
    case "Object": {
      if (!expand) {
        return "{ … }";
      }
      if (shape.fields.length === 0) {
        return "{}";
      }
      return `{ ${shape.fields
        .map(
          (f) =>
            `${f.fieldName}${f.optional ? "?" : ""}: ${shapeToLabel(
              f.shape,
              true,
            )}`,
        )
        .join("; ")} }`;
    }
    case "Union":
      return shape.shapes.map((s) => shapeToLabel(s, expand)).join(" | ");
    case "Boolean":
      return "boolean";
    case "Bytes":
      return "bytes";
    case "Float64":
      return "number";
    case "Int64":
      return "bigint";
    case "Null":
      return "null";
    case "String":
      return "string";
    case "Never":
      return "never";
    default:
      return "unknown";
  }
}

type ObjectShape = Extract<Shape, { type: "Object" }>;

function fieldsFromObjectShape(shape: ObjectShape): SchemaField[] {
  return shape.fields
    .filter((f) => f.fieldName !== "_id" && f.fieldName !== "_creationTime")
    .map((f) => {
      const references = new Set<string>();
      collectReferencesFromShape(f.shape, references);
      const type = shapeToLabel(f.shape);
      return {
        name: f.fieldName,
        type,
        // Only carry the full form when the compact label collapsed something.
        fullType: type.includes("…")
          ? prettifyType(shapeToLabel(f.shape, true))
          : undefined,
        optional: f.optional,
        references: Array.from(references),
      };
    });
}

function fieldsFromShape(shape: Shape): SchemaField[] {
  if (shape.type === "Object") {
    return fieldsFromObjectShape(shape);
  }
  if (shape.type === "Union") {
    return mergeVariantFields(
      shape.shapes.map((s) =>
        s.type === "Object" ? fieldsFromObjectShape(s) : [],
      ),
    );
  }
  return [];
}

function unionFromShape(shape: Shape): SchemaUnion | undefined {
  if (shape.type !== "Union") {
    return undefined;
  }
  const members = shape.shapes;
  if (
    members.length < 2 ||
    !members.every((s): s is ObjectShape => s.type === "Object")
  ) {
    return undefined;
  }
  return {
    variants: members.map((member, i) => ({
      label: `Variant ${i + 1}`,
      fields: fieldsFromObjectShape(member),
    })),
  };
}

// --- Graph assembly ----------------------------------------------------------

function assembleGraph(
  rawNodes: {
    table: string;
    fields: SchemaField[];
    union?: SchemaUnion;
    indexes: SchemaIndex[];
    notInSchema?: boolean;
  }[],
): SchemaGraph {
  const tableNames = new Set(rawNodes.map((n) => n.table));
  const edges: SchemaEdge[] = [];
  const seenEdges = new Set<string>();

  rawNodes.forEach((node) => {
    node.fields.forEach((field) => {
      field.references.forEach((target) => {
        const id = `${node.table}.${field.name}->${target}`;
        if (seenEdges.has(id)) {
          return;
        }
        seenEdges.add(id);
        // Only draw edges to tables that exist in this schema/component.
        if (tableNames.has(target)) {
          edges.push({
            id,
            source: node.table,
            target,
            field: field.name,
            optional: field.optional,
          });
        }
      });
    });
  });

  const nodes: SchemaNode[] = rawNodes.map((node) => ({
    table: node.table,
    fields: node.fields,
    union: node.union,
    indexes: node.indexes,
    notInSchema: node.notInSchema ?? false,
  }));

  return { nodes, edges };
}

// Flatten a table's database, search, and vector indexes into a single list.
function indexesFromTable(table: SchemaJson["tables"][number]): SchemaIndex[] {
  return [
    ...(table.indexes ?? []).map(
      (index): SchemaIndex => ({
        name: index.indexDescriptor,
        kind: "database",
        fields: index.fields,
      }),
    ),
    ...(table.searchIndexes ?? []).map(
      (index): SchemaIndex => ({
        name: index.indexDescriptor,
        kind: "search",
        fields: [index.searchField, ...(index.filterFields ?? [])],
      }),
    ),
    ...(table.vectorIndexes ?? []).map(
      (index): SchemaIndex => ({
        name: index.indexDescriptor,
        kind: "vector",
        fields: [index.vectorField, ...(index.filterFields ?? [])],
      }),
    ),
  ];
}

/**
 * Build a relationship graph from a saved schema. When `shapes` is provided,
 * tables that exist in the data but aren't declared in the schema are merged in
 * (with fields inferred from their shape) and flagged `notInSchema` so the UI
 * can mark them. This keeps the diagram a faithful view of everything in the
 * deployment, not just the declared tables.
 */
export function buildGraphFromSchema(
  schema: SchemaJson,
  shapes?: Map<string, Shape>,
): SchemaGraph {
  const schemaTableNames = new Set(schema.tables.map((t) => t.tableName));
  const rawNodes: {
    table: string;
    fields: SchemaField[];
    union?: SchemaUnion;
    indexes: SchemaIndex[];
    notInSchema?: boolean;
  }[] = schema.tables.map((table) => ({
    table: table.tableName,
    fields: fieldsFromDocumentType(table.documentType),
    union: unionFromDocumentType(table.documentType),
    indexes: indexesFromTable(table),
  }));

  if (shapes) {
    shapes.forEach((shape, table) => {
      // Skip tables already declared in the schema; everything else in the data
      // is a not-in-schema table. Empty tables come back with a `Never` shape
      // (and no inferable fields) — still include them so the diagram matches
      // the data view's table list, which shows empty tables too.
      if (schemaTableNames.has(table)) {
        return;
      }
      rawNodes.push({
        table,
        fields: fieldsFromShape(shape),
        union: unionFromShape(shape),
        indexes: [],
        notInSchema: true,
      });
    });
  }

  rawNodes.sort((a, b) => a.table.localeCompare(b.table));
  return assembleGraph(rawNodes);
}

/**
 * Build a relationship graph from shapes inferred from the data. Includes every
 * table the data view shows — empty tables come back with a `Never` shape (and
 * no inferable fields) but are still rendered as nodes so the diagram matches
 * the data view's table list.
 */
export function buildGraphFromShapes(shapes: Map<string, Shape>): SchemaGraph {
  const rawNodes = Array.from(shapes.entries())
    .map(([table, shape]) => ({
      table,
      fields: fieldsFromShape(shape),
      union: unionFromShape(shape),
      indexes: [] as SchemaIndex[],
    }))
    .sort((a, b) => a.table.localeCompare(b.table));
  return assembleGraph(rawNodes);
}
