// Mirrors Rust's SerializedComponentArgumentValidator.
// The `value` field is a JSON-serialized validator.
type SerializedValidator = { type: "value"; value: string };

// These reflect server types.
export type ComponentDefinitionExport = {
  name: string;
  // how will we figure this out?
  path: string;
  definitionType: {
    type: "childComponent";
    name: string;
  };
  childComponents: [];
  exports: { type: "branch"; branch: [] };
};

// These reflect server types.
// type ComponentDefinitionType
export type ComponentDefinitionType = {
  type: "childComponent";
  name: string;
  args: [string, { type: "value"; value: string }][];
};

// This reflects the SerializedComponentArgument type on the server.
export type SerializedComponentArg =
  | { type: "value"; value: string }
  | { type: "reference"; reference: string }
  | { type: "parentArgument"; name: string }
  | { type: "object"; fields: [string, SerializedComponentArg][] }
  | { type: "array"; elements: SerializedComponentArg[] };
export type AppDefinitionType = { type: "app" };

type ComponentInstantiation = {
  name: string;
  // This is a ComponentPath.
  path: string;
  env: [string, { type: "value"; value: string }][] | null;
  args: [string, SerializedComponentArg][];
};

export type HttpMount = string;

type ComponentExport =
  | { type: "branch"; branch: [string, ComponentExport][] }
  | { type: "leaf"; leaf: string };

// The type expected from the internal .export()
// method of a component or app definition.
export type ComponentDefinitionAnalysis = {
  name: string;
  definitionType: ComponentDefinitionType;
  childComponents: ComponentInstantiation[];
  httpMounts: Record<string, HttpMount>;
  exports: ComponentExport;
  envVars?: [string, SerializedValidator & { optional?: boolean }][];
  init?: string;
};
export type AppDefinitionAnalysis = {
  definitionType: AppDefinitionType;
  // Top-level field (not inside definitionType) to match Rust's
  // SerializedComponentDefinitionMetadata.http_prefix field.
  httpPrefix?: string;
  childComponents: ComponentInstantiation[];
  httpMounts: Record<string, HttpMount>;
  exports: ComponentExport;
  envVars?: [string, SerializedValidator & { optional?: boolean }][];
};
