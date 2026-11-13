import { describe, expect, test } from "vitest";
import {
  functionPaths,
  functionPathsInApiRs,
  pascalCase,
  rustApiCodegen,
  rustIdent,
} from "./rust_api.js";

const object = (value: Record<string, unknown>) =>
  JSON.stringify({ type: "object", value });
const field = (fieldType: unknown, optional = false) => ({
  fieldType,
  optional,
});

function mod(
  functions: Array<{
    name: string;
    udfType?: "Query" | "Mutation" | "Action";
    visibility?: "public" | "internal";
    args?: string | null;
    returns?: string | null;
  }>,
) {
  return {
    functions: functions.map((f) => ({
      name: f.name,
      pos: null,
      udfType: f.udfType ?? "Query",
      visibility: { kind: f.visibility ?? "public" },
      args: f.args ?? null,
      returns: f.returns ?? null,
    })),
    httpRoutes: null,
    cronSpecs: null,
    sourceMapped: null,
  } as any;
}

describe("rustApiCodegen", () => {
  test("typed args from validators, Value without one", () => {
    const out = rustApiCodegen({
      "messages.js": mod([
        { name: "list", args: object({}) },
        {
          name: "add",
          udfType: "Mutation",
          args: object({
            body: field({ type: "string" }),
            author: field({ type: "id", tableName: "users" }),
            likes: field({ type: "number" }, true),
            tags: field({ type: "array", value: { type: "string" } }),
          }),
          returns: JSON.stringify({ type: "id", tableName: "messages" }),
        },
        { name: "untyped" },
      ]),
    });
    expect(out).toContain("pub mod messages {");
    expect(out).toContain("pub struct ListArgs {\n    }");
    expect(out).toContain(
      'pub const list: FunctionRef<ListArgs, Value> =\n        FunctionRef::new("messages:list");',
    );
    expect(out).toContain("pub body: String,");
    expect(out).toContain('/// `Id<"users">`');
    expect(out).toContain(
      '#[serde(default, skip_serializing_if = "Option::is_none")]\n        pub likes: Option<f64>,',
    );
    expect(out).toContain("pub tags: Vec<String>,");
    expect(out).toContain("pub const add: FunctionRef<AddArgs, String>");
    expect(out).toContain("pub const untyped: FunctionRef<Value, Value>");
  });

  test("nested modules, rust modules and internal functions", () => {
    const out = rustApiCodegen({
      "a/b.js": mod([{ name: "f" }]),
      "rust/hello.js": mod([
        {
          name: "hello",
          udfType: "Action",
          args: object({ n: field({ type: "bigint" }) }),
        },
        { name: "secret", udfType: "Action", visibility: "internal" },
      ]),
    });
    expect(out).toContain("pub mod a {");
    expect(out).toContain("    pub mod b {");
    expect(out).toContain('FunctionRef::new("a/b:f")');
    expect(out).toContain("pub mod rust {");
    expect(out).toContain("pub n: Int64,");
    expect(out).toContain('FunctionRef::new("rust/hello:hello")');
    expect(out).toContain("pub mod internal {");
    expect(out).toContain('FunctionRef::new("rust/hello:secret")');
  });

  test("keywords, odd names, nested objects, unions", () => {
    const out = rustApiCodegen({
      "x.js": mod([
        {
          name: "type",
          args: object({
            "my-field": field({ type: "string" }),
            self: field({ type: "boolean" }),
            inner: field({
              type: "object",
              value: { a: field({ type: "null" }) },
            }),
            maybe: field({
              type: "union",
              value: [{ type: "string" }, { type: "null" }],
            }),
            kind: field({
              type: "union",
              value: [
                { type: "literal", value: "a" },
                { type: "literal", value: "b" },
              ],
            }),
            other: field({
              type: "union",
              value: [{ type: "string" }, { type: "number" }],
            }),
          }),
        },
      ]),
    });
    expect(out).toContain("pub const r#type: FunctionRef<TypeArgs, Value>");
    expect(out).toContain(
      '#[serde(rename = "my-field")]\n        pub my_field: String,',
    );
    expect(out).toContain(
      '#[serde(rename = "self")]\n        pub self_: bool,',
    );
    expect(out).toContain("pub inner: TypeArgsInner,");
    expect(out).toContain("pub struct TypeArgsInner {");
    expect(out).toContain("pub maybe: Option<String>,");
    expect(out).toContain("pub kind: String,");
    expect(out).toContain("pub other: Value,");
  });

  test("helpers", () => {
    expect(rustIdent("fn")).toBe("r#fn");
    expect(rustIdent("self")).toBe("self_");
    expect(rustIdent("9lives")).toBe("_9lives");
    expect(pascalCase("list_messages")).toBe("ListMessages");
    expect(pascalCase("listMessages")).toBe("ListMessages");
  });
});

describe("rustApiCodegen returns", () => {
  test("unknown returns are Value; only an explicit null validator is ()", () => {
    const out = rustApiCodegen({
      "m.js": mod([
        { name: "untyped" },
        { name: "unit", returns: JSON.stringify({ type: "null" }) },
        { name: "any", returns: JSON.stringify({ type: "any" }) },
      ]),
    });
    expect(out).toContain("pub const untyped: FunctionRef<Value, Value>");
    expect(out).toContain("pub const unit: FunctionRef<Value, ()>");
    expect(out).toContain("pub const any: FunctionRef<Value, Value>");
  });
});

describe("pre-build api.rs preparation helpers", () => {
  test("functionPathsInApiRs / functionPaths agree with rustApiCodegen", () => {
    const modules = {
      "settings.js": mod([
        {
          name: "get",
          args: object({}),
          returns: JSON.stringify({ type: "number" }),
        },
        { name: "hidden", visibility: "internal" },
      ]),
      "rust/images.js": mod([{ name: "thumbnail", udfType: "Action" }]),
    };
    const out = rustApiCodegen(modules);
    expect(functionPathsInApiRs(out)).toEqual(functionPaths(modules));
    expect([...functionPaths(modules)].sort()).toEqual([
      "rust/images:thumbnail",
      "settings:get",
      "settings:hidden",
    ]);
  });
});
