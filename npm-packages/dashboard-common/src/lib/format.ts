import { Shape } from "shapes";
import jsParser from "prettier-old-sync/parser-babel";
import standalone from "prettier-old-sync/standalone";
import { ObjectFieldType, ValidatorJSON, jsonToConvex } from "convex/values";
import { Index, SearchIndex, VectorIndex } from "convex/server";
import { format } from "date-fns";

type TableDefinition = {
  tableName: string;
  indexes: Index[];
  searchIndexes: SearchIndex[];
  vectorIndexes?: VectorIndex[];
  stagedDbIndexes?: Index[];
  stagedSearchIndexes?: SearchIndex[];
  stagedVectorIndexes?: VectorIndex[];
  documentType: ValidatorJSON | null;
};

export type SchemaJson = {
  tables: TableDefinition[];
  schemaValidation: boolean;
};

export const prettier = (stmt: string, printWidth: number = 60) => {
  try {
    const prettierStmt: string = standalone.format(stmt, {
      plugins: [jsParser],
      printWidth,
      tabWidth: 2,
      semi: true,
      proseWrap: "always",
      parser: "babel-ts",
    });
    return prettierStmt.trim();
  } catch (e: any) {
    if (process.env.NEXT_PUBLIC_ENVIRONMENT === "development") {
      // It's safe to throw the whole error in development.
      throw e;
    }
    // Re-throw with only the first line of the prettier error, which describes the issue
    // Subsequent lines may include source code and should not be logged to our error tracking.
    throw new Error(e.message.split("\n")[0]);
  }
};

// Pretty-prints a JS expression. It’s wrapped in parentheses so that prettier
// parses object literals as expressions instead of blocks; prettier keeps the
// parentheses for some expression types, so they’re stripped afterwards.
export function formatExpression(expression: string, printWidth?: number) {
  const formatted = prettier(`(${expression})`, printWidth).replace(/;$/, "");
  return formatted.startsWith("(") && formatted.endsWith(")")
    ? formatted.slice(1, -1)
    : formatted;
}

export function displaySchemaFromShape({
  shape,
  filterSystemFields = false,
  wrapObjects = true,
}: {
  shape: Shape;
  filterSystemFields?: boolean;
  wrapObjects?: boolean;
}): string {
  function objectFieldToSchema(p: { optional: boolean; shape: Shape }): string {
    if (p.optional) {
      return `v.optional(${displaySchemaFromShape({
        shape: p.shape,
      })})`;
    }
    return `${displaySchemaFromShape({
      shape: p.shape,
    })}`;
  }

  function fieldToSchema(p: {
    fieldName: string;
    optional: boolean;
    shape: Shape;
  }): string {
    return `"${p.fieldName}": ${objectFieldToSchema({
      optional: p.optional,
      shape: p.shape,
    })}`;
  }

  const variant = shape;

  switch (variant.type) {
    case "Array":
      return `v.array(${displaySchemaFromShape({
        shape: variant.shape,
      })})`;
    case "Boolean":
      return `v.boolean()`;
    case "Bytes":
      return `v.bytes()`;
    case "Float64":
      return `v.float64()`;
    case "Id":
      return `v.id("${variant.tableName}")`;
    case "Int64":
      return `v.int64()`;
    case "Never":
      // If the developer saves an empty map, set, or array the shape of the values
      // is Never (we don't know it!). Convert to `v.any()` so the schema is at least functional.
      return `v.any()`;

    case "Null":
      return `v.null()`;
    case "Object": {
      const fieldsStr = filterSystemFields
        ? variant.fields
            // System fields like `_id` and `_creationTime` shouldn't be in schemas
            // (they are automatically added).
            .filter(
              (p) => p.fieldName !== "_id" && p.fieldName !== "_creationTime",
            )
            .map((p) => fieldToSchema(p))
            .join(",\n")
        : variant.fields.map((p) => fieldToSchema(p)).join(",\n");
      return wrapObjects ? `v.object({${fieldsStr}})` : `{${fieldsStr}}`;
    }
    case "Record": {
      const keys = displaySchemaFromShape({
        shape: variant.keyShape,
      });
      const values = objectFieldToSchema(variant.valueShape);
      return `v.record(${keys}, ${values})`;
    }
    case "String":
      return `v.string()`;
    case "Union":
      return `v.union(${variant.shapes
        // When we recurse, still consider this the root because we haven't actually
        // gone into nested objects yet!
        .map((variantShape) =>
          displaySchemaFromShape({
            shape: variantShape,
            filterSystemFields,
          }),
        )
        .join(",")})`;
    case "Unknown":
      return `v.any()`;
    default: {
      // Enforce that the switch-case is exhaustive.
      variant satisfies never;
      throw new Error(`Unrecognized variant ${shape.type} in ${shape}`);
    }
  }
}

function displayDocumentType(validator: ValidatorJSON): string {
  switch (validator.type) {
    case "object":
      return displayObjectSchema(validator.value);
    case "union":
      return displayValidator(validator);
    case "any":
      return displayValidator(validator);
    default:
      throw new Error("Top-level validator must be an object or union.");
  }
}

export function displayObjectFieldSchema(field: ObjectFieldType): string {
  const validator = displayValidator(field.fieldType);
  return field.optional ? `v.optional(${validator})` : validator;
}

function displayObjectSchema(object: Record<string, ObjectFieldType>): string {
  return `{${Object.keys(object)
    .map((key) => {
      const valueType = displayObjectFieldSchema(object[key]);
      return `${key}: ${valueType}`;
    })
    .join(",")}}`;
}

export function displayValidatorPretty(validator: ValidatorJSON): string {
  return prettier(displayValidator(validator));
}

function displayValidator(validator: ValidatorJSON): string {
  switch (validator.type) {
    case "null":
      return `v.null()`;
    case "number":
      return `v.float64()`;
    case "bigint":
      return `v.int64()`;
    case "commitTs":
      return `v.commitTs()`;
    case "boolean":
      return `v.boolean()`;
    case "string":
      return `v.string()`;
    case "bytes":
      return `v.bytes()`;
    case "any":
      return `v.any()`;
    case "literal":
      switch (typeof validator.value) {
        case "string":
          return `v.literal(${JSON.stringify(validator.value)})`;
        case "number":
          return `v.literal(${validator.value})`;
        case "boolean":
          return `v.literal(${validator.value})`;
        case "object":
          return `v.literal(${jsonToConvex(validator.value)}n)`;
        default: {
          throw new Error(
            `Unsupported literal type: ${typeof validator.value}`,
          );
        }
      }
    case "id":
      return `v.id("${validator.tableName}")`;
    case "array":
      return `v.array(${displayValidator(validator.value)})`;
    case "record":
      return `v.record(${displayValidator(
        validator.keys,
      )}, ${displayObjectFieldSchema(validator.values)})`;
    // Deprecated, but could be shown in History tab
    case "set" as any:
    case "map" as any:
      return `v.any()`;
    case "object":
      return `v.object(${displayObjectSchema(validator.value)})`;
    case "union":
      return `v.union(${validator.value
        .map((t: ValidatorJSON) => displayValidator(t))
        .join(",\n")})`;
    default: {
      // Enforce that the switch-case is exhaustive.
      validator satisfies never;
      throw new Error(`Unrecognized validator variant: ${validator}`);
    }
  }
}

function displayIndexes(indexes: Index[], type: "staged" | "active"): string {
  return indexes
    .map((index) => {
      const fields = `[${index.fields
        // Filter out system fields that start with underscore. Ideally these are filtered out in backend (CX-3805), but for now we do it here.
        .filter((field) => (field.length > 0 ? field[0] !== "_" : true))
        .map((field) => `"${field}"`)
        .join(",")}]`;
      return type === "staged"
        ? `.index("${index.indexDescriptor}", { fields: ${fields}, staged: true })`
        : `.index("${index.indexDescriptor}", ${fields})`;
    })
    .join("");
}

function displaySearchIndexes(
  searchIndexes: SearchIndex[],
  type: "staged" | "active",
): string {
  return searchIndexes
    .map(
      (searchIndex) =>
        `.searchIndex("${searchIndex.indexDescriptor}", {searchField: "${
          searchIndex.searchField
        }"
        ${appendFilterFieldsOrEmpty(searchIndex)}
        ${appendStagedOrEmpty(type)}})`,
    )
    .join("");
}

function appendFilterFieldsOrEmpty(index: SearchIndex | VectorIndex): string {
  return index.filterFields.length > 0
    ? `, filterFields: [${index.filterFields
        .map((field) => `"${field}"`)
        .join(",")}]`
    : "";
}

function appendStagedOrEmpty(type: "staged" | "active"): string {
  return type === "staged" ? ", staged: true" : "";
}

function displayVectorIndexes(
  vectorIndexes: VectorIndex[],
  type: "staged" | "active",
): string {
  return vectorIndexes
    .map(
      (vectorIndex) => `.vectorIndex("${vectorIndex.indexDescriptor}"
        , {vectorField: "${vectorIndex.vectorField}"
        , dimensions: ${vectorIndex.dimensions}
        ${appendFilterFieldsOrEmpty(vectorIndex)}
        ${appendStagedOrEmpty(type)}
      })`,
    )
    .join("");
}

function displayTableDefinition(tableDefinition: TableDefinition): string {
  const documentType = displayDocumentType(
    tableDefinition.documentType ?? { type: "any" },
  );
  return `${tableDefinition.tableName}: defineTable(${documentType}
  )${
    displayIndexes(tableDefinition.indexes, "active") +
    displayIndexes(tableDefinition.stagedDbIndexes ?? [], "staged") +
    displaySearchIndexes(tableDefinition.searchIndexes, "active") +
    displaySearchIndexes(tableDefinition.stagedSearchIndexes ?? [], "staged") +
    displayVectorIndexes(tableDefinition.vectorIndexes ?? [], "active") +
    displayVectorIndexes(tableDefinition.stagedVectorIndexes ?? [], "staged")
  }`;
}

export function displaySchema(schema: SchemaJson, relativePath = "") {
  const tables = schema.tables.map((table: any) =>
    displayTableDefinition(table),
  );
  return displayTableSchemas(tables, {
    schemaValidation: schema.schemaValidation,
    relativePath,
  });
}

export function displaySchemaFromShapes(
  shapes: Map<string, Shape>,
): string | undefined {
  const tables: string[] = [];
  shapes.forEach((shape, table) => {
    if (shape.type !== "Never") {
      const tableShape = displaySchemaFromShape({
        shape,
        filterSystemFields: true,
        wrapObjects: false,
      });
      tables.push(`"${table}": defineTable(${tableShape})`);
    }
  });

  return displayTableSchemas(tables, {
    schemaValidation: true,
    relativePath: "",
  });
}

function displayTableSchemas(
  tables: string[],
  options: {
    schemaValidation: boolean;
    relativePath: string;
  },
): string | undefined {
  if (!tables.length) {
    return undefined;
  }
  const imports = `
    import { defineSchema, defineTable } from "${options.relativePath}convex/server";
    import { v } from "${options.relativePath}convex/values"`;

  // schemaValidation is a default true flag, so we only make it false if it is explicitly set to false.
  // undefined => true.
  const schemaOptions =
    options.schemaValidation === false ? ", { schemaValidation: false }" : "";
  return prettier(`
    ${imports}

    export default defineSchema({${tables.join(",")}}${schemaOptions});`);
}

// Intl.NumberFormat uses B instead of G for billion prefix, so handroll
export function formatBytes(number: number): string;
export function formatBytes(number: number | null): string | null;
export function formatBytes(number: number | null) {
  const nonBreakingSpace = String.fromCharCode(160);
  if (number === null) {
    return null;
  }
  if (number === 0) {
    return `0${nonBreakingSpace}B`;
  }

  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];

  const i = Math.floor(Math.log(number) / Math.log(k));

  // parseFloat trick gets rid of trailing zeroes
  return `${parseFloat((number / k ** i).toFixed(2))}${nonBreakingSpace}${
    sizes[i]
  }`;
}

const NUMBER_FORMAT = new Intl.NumberFormat();
export function formatNumber(value: number): string;
export function formatNumber(value: number | null): string | null;
export function formatNumber(value: number | null): string | null {
  if (value === null) return null;
  return NUMBER_FORMAT.format(value);
}

const NUMBER_FORMAT_COMPACT = new Intl.NumberFormat("en-US", {
  notation: "compact",
  compactDisplay: "short",
  roundingMode: "trunc",
});
// Compact formatter that keeps up to `maximumFractionDigits` decimals. Default
// compact notation drops the fractional part once the magnitude reaches ~2
// significant digits (12.34 -> "12"); passing a precision preserves it
// (12.34 -> "12.34"). Cached per precision.
const NUMBER_FORMAT_COMPACT_WITH_DECIMALS = new Map<
  number,
  Intl.NumberFormat
>();
export function formatNumberCompact(
  value: number | bigint,
  maximumFractionDigits?: number,
): string;
export function formatNumberCompact(
  value: number | bigint | null,
  maximumFractionDigits?: number,
): string | null;
export function formatNumberCompact(
  value: number | bigint | null,
  maximumFractionDigits?: number,
): string | null {
  if (value === null) return null;
  let formatter = NUMBER_FORMAT_COMPACT;
  if (maximumFractionDigits !== undefined) {
    let cached = NUMBER_FORMAT_COMPACT_WITH_DECIMALS.get(maximumFractionDigits);
    if (cached === undefined) {
      cached = new Intl.NumberFormat("en-US", {
        notation: "compact",
        compactDisplay: "short",
        maximumFractionDigits,
        roundingMode: "trunc",
      });
      NUMBER_FORMAT_COMPACT_WITH_DECIMALS.set(maximumFractionDigits, cached);
    }
    formatter = cached;
  }
  const formatted = formatter.format(value);
  // Intl renders -0 (and tiny negatives that round to zero) as "-0"; never
  // surface a signed zero.
  return formatted === "-0" ? "0" : formatted;
}

export function msFormat(n: number): string {
  if (n > 1000) {
    let s = `${Math.floor(n / 100) / 10}`;
    if (!s.includes(".")) {
      s = `${s}.0`;
    }
    s = `${s}s`;
    return s;
  }
  return `${Math.floor(n)}ms`;
}

export function formatDateTime(date: Date): string {
  return format(date, "MMM dd, HH:mm:ss");
}

export function formatDate(date: Date): string {
  return format(date, "MMMM dd, yyyy");
}

// Renders the calendar date an instant falls on in UTC, matching
// `formatDate`'s shape. Use it for dates a backend pinned to UTC — billing
// dates from Orb, say — which `formatDate` would render as the previous day
// for anyone west of UTC.
export function formatUtcDate(date: Date): string {
  return new Intl.DateTimeFormat("en-US", {
    timeZone: "UTC",
    month: "long",
    day: "2-digit",
    year: "numeric",
  }).format(date);
}

export function toNumericUTC(dateString: string) {
  // Parsing manually the date to use UTC.
  const [year, month, day] = dateString.split("-");
  return Date.UTC(Number(year), Number(month) - 1, Number(day));
}

export const THIRTY_MINUTES_MS = 30 * 60 * 1000;

export function toDateTimeLocalValue(
  d: Date,
  options?: { includeSeconds?: boolean },
): string {
  const year = d.getFullYear();
  const month = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  const hours = String(d.getHours()).padStart(2, "0");
  const minutes = String(d.getMinutes()).padStart(2, "0");
  const base = `${year}-${month}-${day}T${hours}:${minutes}`;
  if (!options?.includeSeconds) return base;
  const seconds = String(d.getSeconds()).padStart(2, "0");
  return `${base}:${seconds}`;
}

export function toDateInputValue(d: Date): string {
  return toDateTimeLocalValue(d).slice(0, 10);
}

export const timeLabelForMinute = (value: string | number | undefined) => {
  // Handle undefined, null, or empty values
  if (value === undefined || value === null || value === "") {
    return "";
  }

  // Convert number to string
  const stringValue = typeof value === "number" ? String(value) : value;

  // TODO(ari): Consolidate all the time rendering logic - this is a hack
  // for now
  if (stringValue.includes("-") || !stringValue.includes(":")) {
    return stringValue;
  }
  const [time, modifier] = stringValue.split(" ");
  const [hours, minutes] = time.split(":");
  const date = new Date();
  const hourValue = parseInt(hours);

  // Handle 12-hour to 24-hour conversion
  let hour24 = hourValue;
  if (modifier === "PM" && hourValue !== 12) {
    hour24 = hourValue + 12;
  } else if (modifier === "AM" && hourValue === 12) {
    hour24 = 0;
  }

  date.setHours(hour24);
  date.setMinutes(parseInt(minutes));
  const oneMinuteLater = new Date(date);
  oneMinuteLater.setMinutes(date.getMinutes() + 1);

  return `${formatTime(date)} – ${formatTime(oneMinuteLater)}`;
};

const formatTime = (date: Date) => {
  let hours = date.getHours();
  const minutes = date.getMinutes();
  const ampm = hours >= 12 ? "PM" : "AM";
  hours %= 12;
  hours = hours || 12; // the hour '0' should be '12'
  const strMinutes = minutes < 10 ? `0${minutes}` : minutes;
  return `${hours}:${strMinutes} ${ampm}`;
};
