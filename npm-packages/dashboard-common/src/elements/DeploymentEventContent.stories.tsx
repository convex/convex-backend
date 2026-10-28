import { Infer } from "convex/values";
import { integrationAuditConfig } from "system-udfs/convex/tableDefs/deploymentAuditLogTable";
import { Meta, StoryObj } from "@storybook/nextjs";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { DeploymentEventContent } from "@common/elements/DeploymentEventContent";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { SchemaJson } from "@common/lib/format";
import { Sheet } from "@ui/Sheet";

/** Helper to make sure the serialized values in the story are correct */
function schemaJsonAsString(value: SchemaJson): string {
  return JSON.stringify(value);
}

const meta = {
  component: DeploymentEventContent,
  decorators: [
    (Story) => (
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <Sheet>
          <Story />
        </Sheet>
      </DeploymentInfoContext.Provider>
    ),
  ],
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof DeploymentEventContent>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CreateEnvironmentVariable: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "create_environment_variable",
      metadata: {
        variable_name: "envVar",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteEnvironmentVariable: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_environment_variable",
      metadata: {
        variable_name: "envVar",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const UpdateEnvironmentVariable: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "update_environment_variable",
      metadata: {
        variable_name: "envVar",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const ReplaceEnvironmentVariable: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "replace_environment_variable",
      metadata: {
        previous_variable_name: "envVar",
        variable_name: "envVar2",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const UpdateIndexes: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "build_indexes",
      metadata: {
        added_indexes: [
          { name: "my_index", type: "database", fields: ["field1", "field2"] },
          {
            name: "my_search_index",
            type: "search",
            searchField: "field",
            filterFields: ["field1", "field2"],
          },
        ],
        removed_indexes: [
          { name: "my_index", type: "database", fields: ["field1", "field2"] },
          {
            name: "my_search_index",
            type: "search",
            searchField: "field",
            filterFields: ["field1", "field2"],
          },
        ],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PausingDeploymentDeprecated: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "change_deployment_state",
      metadata: {
        old_state: "running",
        new_state: "paused",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const ResumingDeploymentDeprecated: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "change_deployment_state",
      metadata: {
        old_state: "paused",
        new_state: "running",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const UserUnpausedDeployment: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "unpause_deployment",
      metadata: {},
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};
export const UserPausedDeployment: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "pause_deployment",
      metadata: {},
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const SystemUnblockedDeployment: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "change_system_stop_state",
      metadata: {
        old_state: "disabled",
        new_state: "none",
      },
      memberName: "System",
      member_id: null,
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const SystemDisabledDeployment: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "change_system_stop_state",
      metadata: {
        old_state: "none",
        new_state: "disabled",
      },
      memberName: "System",
      member_id: null,
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const SystemSuspendedDeployment: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "change_system_stop_state",
      metadata: {
        old_state: "none",
        new_state: "suspended",
      },
      memberName: "System",
      member_id: null,
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfig: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: { added: [], removed: [] },
        server_version: null,
        modules: { added: [], removed: [] },
        crons: { added: [], updated: [], deleted: [] },
        schema: null,
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithVersionChange: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: { added: [], removed: [] },
        server_version: { previous_version: "0.5.0", next_version: "0.60" },
        modules: { added: [], removed: [] },
        crons: { added: [], updated: [], deleted: [] },
        schema: null,
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithAuthChange: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: { added: ["auth1", "auth2"], removed: ["auth3", "auth4"] },
        server_version: null,
        modules: { added: [], removed: [] },
        crons: { added: [], updated: [], deleted: [] },
        schema: null,
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithChange: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: { added: [], removed: [] },
        server_version: null,
        modules: { added: [], removed: [] },
        crons: { added: [], updated: [], deleted: [] },
        schema: {
          previous_schema_id: "" as Id<"_schemas">,
          previous_schema: schemaJsonAsString({
            tables: [
              {
                tableName: "messages",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    author: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    body: { fieldType: { type: "string" }, optional: false },
                    isRemoved: {
                      fieldType: {
                        type: "object",
                        value: {
                          booleanOrNumber: {
                            fieldType: {
                              type: "union",
                              value: [{ type: "boolean" }, { type: "number" }],
                            },
                            optional: false,
                          },
                          nestedObject: {
                            fieldType: {
                              type: "object",
                              value: {
                                property: {
                                  fieldType: { type: "string" },
                                  optional: false,
                                },
                              },
                            },
                            optional: false,
                          },
                          numbers: {
                            fieldType: {
                              type: "array",
                              value: { type: "number" },
                            },
                            optional: false,
                          },
                          string: {
                            fieldType: { type: "string" },
                            optional: true,
                          },
                        },
                      },
                      optional: false,
                    },
                  },
                },
              },
            ],
            schemaValidation: true,
          }),
          next_schema_id: "" as Id<"_schemas">,
          next_schema: schemaJsonAsString({
            tables: [
              {
                tableName: "messages",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    author: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    body: { fieldType: { type: "string" }, optional: false },
                    isRemoved: {
                      fieldType: {
                        type: "object",
                        value: {
                          booleanOrNumber: {
                            fieldType: {
                              type: "union",
                              value: [{ type: "boolean" }, { type: "number" }],
                            },
                            optional: false,
                          },
                          nestedObject: {
                            fieldType: {
                              type: "object",
                              value: {
                                property: {
                                  fieldType: {
                                    type: "array",
                                    value: {
                                      type: "union",
                                      value: [
                                        { type: "string" },
                                        { type: "bigint" },
                                        { type: "null" },
                                      ],
                                    },
                                  },
                                  optional: false,
                                },
                              },
                            },
                            optional: false,
                          },
                          numbers: {
                            fieldType: {
                              type: "array",
                              value: { type: "number" },
                            },
                            optional: false,
                          },
                          string: {
                            fieldType: { type: "string" },
                            optional: true,
                          },
                        },
                      },
                      optional: false,
                    },
                  },
                },
              },
            ],
            schemaValidation: true,
          }),
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithAddition: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: { added: [], removed: [] },
        server_version: null,
        modules: { added: [], removed: [] },
        crons: { added: [], updated: [], deleted: [] },
        schema: {
          previous_schema_id: null,
          previous_schema: null,
          next_schema_id: "" as Id<"_schemas">,
          next_schema: schemaJsonAsString({
            tables: [
              {
                tableName: "messages",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    author: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    body: { fieldType: { type: "string" }, optional: false },
                    isRemoved: {
                      fieldType: {
                        type: "object",
                        value: {
                          booleanOrNumber: {
                            fieldType: {
                              type: "union",
                              value: [{ type: "boolean" }, { type: "number" }],
                            },
                            optional: false,
                          },
                          nestedObject: {
                            fieldType: {
                              type: "object",
                              value: {
                                property: {
                                  fieldType: {
                                    type: "array",
                                    value: {
                                      type: "union",
                                      value: [
                                        { type: "string" },
                                        { type: "bigint" },
                                        { type: "null" },
                                      ],
                                    },
                                  },
                                  optional: false,
                                },
                              },
                            },
                            optional: false,
                          },
                          numbers: {
                            fieldType: {
                              type: "array",
                              value: { type: "number" },
                            },
                            optional: false,
                          },
                          string: {
                            fieldType: { type: "string" },
                            optional: true,
                          },
                        },
                      },
                      optional: false,
                    },
                  },
                },
              },
            ],
            schemaValidation: true,
          }),
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithDeletion: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: { added: [], removed: [] },
        server_version: null,
        modules: { added: [], removed: [] },
        crons: { added: [], updated: [], deleted: [] },
        schema: {
          previous_schema_id: "" as Id<"_schemas">,
          previous_schema: schemaJsonAsString({
            tables: [
              {
                tableName: "messages",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    author: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    body: { fieldType: { type: "string" }, optional: false },
                    isRemoved: {
                      fieldType: {
                        type: "object",
                        value: {
                          booleanOrNumber: {
                            fieldType: {
                              type: "union",
                              value: [{ type: "boolean" }, { type: "number" }],
                            },
                            optional: false,
                          },
                          nestedObject: {
                            fieldType: {
                              type: "object",
                              value: {
                                property: {
                                  fieldType: { type: "string" },
                                  optional: false,
                                },
                              },
                            },
                            optional: false,
                          },
                          numbers: {
                            fieldType: {
                              type: "array",
                              value: { type: "number" },
                            },
                            optional: false,
                          },
                          string: {
                            fieldType: { type: "string" },
                            optional: true,
                          },
                        },
                      },
                      optional: false,
                    },
                  },
                },
              },
            ],
            schemaValidation: true,
          }),
          next_schema_id: null,
          next_schema: null,
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithEnforcementChange: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: { added: [], removed: [] },
        server_version: null,
        modules: { added: [], removed: [] },
        crons: { added: [], updated: [], deleted: [] },
        schema: {
          previous_schema_id: "" as Id<"_schemas">,
          previous_schema: schemaJsonAsString({
            tables: [
              {
                tableName: "messages",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    author: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    body: { fieldType: { type: "string" }, optional: false },
                  },
                },
              },
            ],
            schemaValidation: false,
          }),
          next_schema_id: "" as Id<"_schemas">,
          next_schema: schemaJsonAsString({
            tables: [
              {
                tableName: "messages",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    author: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    body: { fieldType: { type: "string" }, optional: false },
                  },
                },
              },
            ],
            schemaValidation: true,
          }),
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithLargeDiff: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: {
          added: [],
          removed: [],
        },
        crons: {
          added: [],
          deleted: [],
          updated: [],
        },
        modules: {
          added: [],
          removed: [],
        },
        schema: {
          next_schema: schemaJsonAsString({
            tables: [
              {
                tableName: "games",
                indexes: [
                  {
                    indexDescriptor: "s",
                    fields: ["slug", "_creationTime"],
                  },
                ],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    hostId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: false,
                    },
                    nextGameId: {
                      fieldType: { type: "id", tableName: "games" },
                      optional: true,
                    },
                    playerIds: {
                      fieldType: {
                        type: "array",
                        value: { type: "id", tableName: "users" },
                      },
                      optional: false,
                    },
                    roundIds: {
                      fieldType: {
                        type: "array",
                        value: { type: "id", tableName: "rounds" },
                      },
                      optional: false,
                    },
                    slug: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    state: {
                      fieldType: {
                        type: "union",
                        value: [
                          {
                            type: "object",
                            value: {
                              stage: {
                                fieldType: {
                                  type: "union",
                                  value: [
                                    { type: "literal", value: "lobby" },
                                    { type: "literal", value: "generate" },
                                    { type: "literal", value: "recap" },
                                  ],
                                },
                                optional: false,
                              },
                            },
                          },
                          {
                            type: "object",
                            value: {
                              roundId: {
                                fieldType: {
                                  type: "id",
                                  tableName: "rounds",
                                },
                                optional: false,
                              },
                              stage: {
                                fieldType: {
                                  type: "literal",
                                  value: "rounds",
                                },
                                optional: false,
                              },
                            },
                          },
                        ],
                      },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "publicGame",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    roundId: {
                      fieldType: { type: "id", tableName: "rounds" },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "rounds",
                indexes: [
                  {
                    indexDescriptor: "public_game",
                    fields: [
                      "publicRound",
                      "stage",
                      "lastUsed",
                      "_creationTime",
                    ],
                  },
                ],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    authorId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: false,
                    },
                    imageStorageId: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    lastUsed: {
                      fieldType: { type: "number" },
                      optional: true,
                    },
                    publicRound: {
                      fieldType: { type: "boolean" },
                      optional: true,
                    },
                    stage: {
                      fieldType: {
                        type: "union",
                        value: [
                          { type: "literal", value: "label" },
                          { type: "literal", value: "guess" },
                          { type: "literal", value: "reveal" },
                        ],
                      },
                      optional: false,
                    },
                    stageEnd: {
                      fieldType: { type: "number" },
                      optional: false,
                    },
                    stageStart: {
                      fieldType: { type: "number" },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "sessions",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    gameIds: {
                      fieldType: {
                        type: "array",
                        value: { type: "id", tableName: "games" },
                      },
                      optional: false,
                    },
                    submissionIds: {
                      fieldType: {
                        type: "array",
                        value: { type: "id", tableName: "submissions" },
                      },
                      optional: false,
                    },
                    userId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "submissions",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    authorId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: false,
                    },
                    prompt: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    result: {
                      fieldType: {
                        type: "union",
                        value: [
                          {
                            type: "object",
                            value: {
                              elapsedMs: {
                                fieldType: { type: "number" },
                                optional: false,
                              },
                              status: {
                                fieldType: {
                                  type: "literal",
                                  value: "waiting",
                                },
                                optional: false,
                              },
                            },
                          },
                          {
                            type: "object",
                            value: {
                              details: {
                                fieldType: { type: "string" },
                                optional: false,
                              },
                              status: {
                                fieldType: {
                                  type: "literal",
                                  value: "generating",
                                },
                                optional: false,
                              },
                            },
                          },
                          {
                            type: "object",
                            value: {
                              elapsedMs: {
                                fieldType: { type: "number" },
                                optional: false,
                              },
                              reason: {
                                fieldType: { type: "string" },
                                optional: false,
                              },
                              status: {
                                fieldType: {
                                  type: "literal",
                                  value: "failed",
                                },
                                optional: false,
                              },
                            },
                          },
                          {
                            type: "object",
                            value: {
                              elapsedMs: {
                                fieldType: { type: "number" },
                                optional: false,
                              },
                              imageStorageId: {
                                fieldType: { type: "string" },
                                optional: false,
                              },
                              status: {
                                fieldType: {
                                  type: "literal",
                                  value: "saved",
                                },
                                optional: false,
                              },
                            },
                          },
                        ],
                      },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "users",
                indexes: [
                  {
                    indexDescriptor: "by_token",
                    fields: ["tokenIdentifier", "_creationTime"],
                  },
                ],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    claimedByUserId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: true,
                    },
                    name: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    password_hash: {
                      fieldType: { type: "string" },
                      optional: true,
                    },
                    pictureUrl: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    tokenIdentifier: {
                      fieldType: { type: "string" },
                      optional: true,
                    },
                  },
                },
              },
            ],
            schemaValidation: true,
          }),
          next_schema_id: "" as Id<"_schemas">,
          previous_schema: schemaJsonAsString({
            tables: [
              {
                tableName: "games",
                indexes: [
                  {
                    indexDescriptor: "s",
                    fields: ["slug", "_creationTime"],
                  },
                ],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    hostId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: false,
                    },
                    nextGameId: {
                      fieldType: { type: "id", tableName: "games" },
                      optional: true,
                    },
                    playerIds: {
                      fieldType: {
                        type: "array",
                        value: { type: "id", tableName: "users" },
                      },
                      optional: false,
                    },
                    roundIds: {
                      fieldType: {
                        type: "array",
                        value: { type: "id", tableName: "rounds" },
                      },
                      optional: false,
                    },
                    slug: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    state: {
                      fieldType: {
                        type: "union",
                        value: [
                          {
                            type: "object",
                            value: {
                              stage: {
                                fieldType: {
                                  type: "union",
                                  value: [
                                    { type: "literal", value: "lobby" },
                                    { type: "literal", value: "generate" },
                                    { type: "literal", value: "recap" },
                                  ],
                                },
                                optional: false,
                              },
                            },
                          },
                          {
                            type: "object",
                            value: {
                              roundId: {
                                fieldType: {
                                  type: "id",
                                  tableName: "rounds",
                                },
                                optional: false,
                              },
                              stage: {
                                fieldType: {
                                  type: "literal",
                                  value: "rounds",
                                },
                                optional: false,
                              },
                            },
                          },
                        ],
                      },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "publicGame",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    roundId: {
                      fieldType: { type: "id", tableName: "rounds" },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "rounds",
                indexes: [
                  {
                    indexDescriptor: "public_game",
                    fields: [
                      "publicRound",
                      "stage",
                      "lastUsed",
                      "_creationTime",
                    ],
                  },
                ],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    authorId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: false,
                    },
                    imageStorageId: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    lastUsed: {
                      fieldType: { type: "number" },
                      optional: true,
                    },
                    options: {
                      fieldType: {
                        type: "array",
                        value: {
                          type: "object",
                          value: {
                            authorId: {
                              fieldType: { type: "id", tableName: "users" },
                              optional: false,
                            },
                            likes: {
                              fieldType: {
                                type: "array",
                                value: { type: "id", tableName: "users" },
                              },
                              optional: false,
                            },
                            prompt: {
                              fieldType: { type: "string" },
                              optional: false,
                            },
                            votes: {
                              fieldType: {
                                type: "array",
                                value: { type: "id", tableName: "users" },
                              },
                              optional: false,
                            },
                          },
                        },
                      },
                      optional: false,
                    },
                    publicRound: {
                      fieldType: { type: "boolean" },
                      optional: true,
                    },
                    stage: {
                      fieldType: {
                        type: "union",
                        value: [
                          { type: "literal", value: "label" },
                          { type: "literal", value: "guess" },
                          { type: "literal", value: "reveal" },
                        ],
                      },
                      optional: false,
                    },
                    stageEnd: {
                      fieldType: { type: "number" },
                      optional: false,
                    },
                    stageStart: {
                      fieldType: { type: "number" },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "sessions",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    gameIds: {
                      fieldType: {
                        type: "array",
                        value: { type: "id", tableName: "games" },
                      },
                      optional: false,
                    },
                    submissionIds: {
                      fieldType: {
                        type: "array",
                        value: { type: "id", tableName: "submissions" },
                      },
                      optional: false,
                    },
                    userId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "submissions",
                indexes: [],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    authorId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: false,
                    },
                    prompt: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                  },
                },
              },
              {
                tableName: "users",
                indexes: [
                  {
                    indexDescriptor: "by_token",
                    fields: ["tokenIdentifier", "_creationTime"],
                  },
                ],
                searchIndexes: [],
                documentType: {
                  type: "object",
                  value: {
                    claimedByUserId: {
                      fieldType: { type: "id", tableName: "users" },
                      optional: true,
                    },
                    name: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    password_hash: {
                      fieldType: { type: "string" },
                      optional: true,
                    },
                    pictureUrl: {
                      fieldType: { type: "string" },
                      optional: false,
                    },
                    tokenIdentifier: {
                      fieldType: { type: "string" },
                      optional: true,
                    },
                  },
                },
              },
            ],
            schemaValidation: true,
          }),
          previous_schema_id: "" as Id<"_schemas">,
        },
        server_version: null,
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithCron: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config",
      metadata: {
        auth: { added: [], removed: [] },
        server_version: null,
        modules: { added: [], removed: [] },
        crons: {
          added: ["added cron job 1", "added cron job 2", "added cron job 3"],
          deleted: ["removed cron 1", "removed cron 2", "removed cron 3"],
          updated: ["updated cron 1", "updated cron 2", "updated cron 3"],
        },
        schema: null,
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const SnapshotImportZip: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "snapshot_import",
      metadata: {
        table_names: [
          {
            table_names: ["_storage", "users", "friendships"],
            component: null,
          },
        ],
        table_count: BigInt(4),
        import_mode: "Replace",
        import_format: { format: "zip" },
        requestor: {
          type: "snapshotImport",
        },
        table_names_deleted: [],
        table_count_deleted: BigInt(0),
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const SnapshotImportCsv: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "snapshot_import",
      metadata: {
        table_names: [
          {
            table_names: ["users"],
            component: null,
          },
        ],
        table_count: BigInt(4),
        import_mode: "RequireEmpty",
        import_format: { format: "csv", table: "users" },
        requestor: {
          type: "snapshotImport",
        },
        table_names_deleted: [],
        table_count_deleted: BigInt(0),
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteScheduledJobsTable: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_scheduled_jobs_table",
      metadata: {
        component_id: null,
        component: null,
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteTables: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_tables",
      metadata: {
        component_id: null,
        component: null,
        table_names: ["users", "messages"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteSingleTable: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_tables",
      metadata: {
        component_id: null,
        component: null,
        table_names: ["users"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteTablesInComponent: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_tables",
      metadata: {
        component_id: "abc123",
        component: "chat",
        table_names: ["messages"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteComponent: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_component",
      metadata: {
        component_id: "abc123",
        component: "ratelimit",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const CancelAllScheduledFunctions: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "cancel_all_scheduled_functions",
      metadata: {
        component_id: null,
        component: null,
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const CancelScheduledFunction: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "cancel_scheduled_function",
      metadata: {
        component_id: null,
        component: null,
        scheduled_function_id: "jd72k9x8nmgz5h3qawc4btep0n6y1fvr",
        function_path: "messages:sendDelayed",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const RequestExport: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "request_export",
      metadata: {
        id: "jd72k9x8nmgz5h3qawc4btep0n6y1fvr",
        component_id: null,
        component: null,
        format: "zip",
        requestor: "dashboard",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const CreateDataSync: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "create_data_sync",
      metadata: {
        sync_id: "fivetran-8b6c0f7e-4a3d-4a20-9c47-2f6d1f0a5b3c",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DownloadExport: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "download_export",
      metadata: {
        id: "jd72k9x8nmgz5h3qawc4btep0n6y1fvr",
        component_id: null,
        component: null,
        format: "zip",
        requestor: "dashboard",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
    },
  },
};

export const CancelExport: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "cancel_export",
      metadata: {
        id: "jd72k9x8nmgz5h3qawc4btep0n6y1fvr",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const SetExportExpiration: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "set_export_expiration",
      metadata: {
        id: "jd72k9x8nmgz5h3qawc4btep0n6y1fvr",
        expiration_ts_ms: BigInt(1703030400000),
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const CreateIntegration: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "create_integration",
      metadata: {
        id: "jd72k9x8nmgz5h3qawc4btep0n6y1fvr",
        type: "datadog",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const UpdateIntegration: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "update_integration",
      metadata: {
        id: "jd72k9x8nmgz5h3qawc4btep0n6y1fvr",
        type: "axiom",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteIntegration: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_integration",
      metadata: {
        id: "jd72k9x8nmgz5h3qawc4btep0n6y1fvr",
        type: "webhook",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

const integrationAuditEvent = {
  _id: "" as Id<"_deployment_audit_log">,
  _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
  memberName: "member@convex.dev",
  member_id: BigInt(1),
  token_id: BigInt(1),
  app_client_id: null,
};

const s3AuditConfig = {
  type: "s3Export" as const,
  bucket: "analytics-bucket",
  region: "us-east-1",
  prefix: "exports/",
  period: "daily" as const,
  selection: { _other: "included" as const },
};

export const CreateS3ExportIntegration: Story = {
  args: {
    event: {
      ...integrationAuditEvent,
      action: "create_integration",
      metadata: { id: "s3-export-id", type: "s3Export", config: s3AuditConfig },
    },
  },
};

export const UpdateS3ExportIntegration: Story = {
  args: {
    event: {
      ...integrationAuditEvent,
      action: "update_integration",
      metadata: {
        id: "s3-export-id",
        type: "s3Export",
        previous: s3AuditConfig,
        current: {
          ...s3AuditConfig,
          bucket: "new-analytics-bucket",
          prefix: null,
          period: "hourly",
          selection: {
            _other: "excluded",
            "": {
              _other: "excluded",
              users: { _other: "included", ssn: "excluded" },
            },
          },
        },
        credential_changes: {
          type: "s3Export",
          access_key_id_changed: false,
          secret_access_key_changed: true,
        },
      },
    },
  },
};

export const DeleteS3ExportIntegration: Story = {
  args: {
    event: {
      ...integrationAuditEvent,
      action: "delete_integration",
      metadata: { id: "s3-export-id", type: "s3Export", config: s3AuditConfig },
    },
  },
};

function createIntegrationStory(
  config: Infer<typeof integrationAuditConfig>,
): Story {
  return {
    args: {
      event: {
        ...integrationAuditEvent,
        action: "create_integration",
        metadata: { id: "integration-id", type: config.type, config },
      },
    },
  };
}

export const CreateDatadogWithConfiguration = createIntegrationStory({
  type: "datadog",
  site: "EU",
  service: "backend",
  tags: ["env:prod"],
  format_version: "2",
  topics: ["console", "custom_audit"],
});

export const CreateAxiomWithConfiguration = createIntegrationStory({
  type: "axiom",
  dataset: "deployment-logs",
  ingest_origin: "https://api.axiom.co",
  attributes: [{ key: "env", value: "prod" }],
  format_version: "2",
  topics: null,
});

export const CreateWebhookWithConfiguration = createIntegrationStory({
  type: "webhook",
  url_origin: "https://hooks.example.com:8443",
  format: "jsonl",
  topics: ["custom_audit"],
});

export const CreateSentryWithConfiguration = createIntegrationStory({
  type: "sentry",
  dsn_origin: "https://sentry.example.com",
  project_id: "42",
  tags: { env: "prod" },
  format_version: "2",
});

export const CreatePostHogLogsWithConfiguration = createIntegrationStory({
  type: "postHogLogs",
  host_origin: "https://eu.i.posthog.com",
  service_name: "backend",
  topics: null,
});

export const CreatePostHogErrorTrackingWithConfiguration =
  createIntegrationStory({
    type: "postHogErrorTracking",
    host_origin: "https://us.i.posthog.com",
  });

export const UpdateWebhookCredentials: Story = {
  args: {
    event: {
      ...integrationAuditEvent,
      action: "update_integration",
      metadata: {
        id: "webhook-id",
        type: "webhook",
        previous: {
          type: "webhook",
          url_origin: "https://hooks.example.com",
          format: "jsonl",
          topics: null,
        },
        current: {
          type: "webhook",
          url_origin: "https://hooks.example.com",
          format: "jsonl",
          topics: null,
        },
        credential_changes: {
          type: "webhook",
          hmac_secret_changed: true,
          url_changed: true,
        },
      },
    },
  },
};

export const AddDocuments: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "add_documents",
      metadata: {
        component_id: null,
        component: null,
        table: "users",
        document_ids: ["doc1", "doc2", "doc3"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const AddDocumentsInComponent: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "add_documents",
      metadata: {
        component_id: "abc123",
        component: "chat/messages",
        table: "messages",
        document_ids: ["doc1"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteDocuments: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_documents",
      metadata: {
        component_id: null,
        component: null,
        table: "users",
        document_ids: ["doc1", "doc2"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const UpdateSingleDocument: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "update_documents",
      metadata: {
        component_id: null,
        component: null,
        table: "users",
        document_ids: ["doc1"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const UpdateDocumentsWithTable: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "update_documents",
      metadata: {
        component_id: null,
        component: null,
        table: "messages",
        document_ids: ["doc1", "doc2", "doc3", "doc4"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const CreateTable: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "create_table",
      metadata: {
        component_id: null,
        component: null,
        table: "new_table",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteFiles: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_files",
      metadata: {
        component_id: null,
        component: null,
        storage_ids: ["stor1", "stor2", "stor3"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteSingleFile: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_files",
      metadata: {
        component_id: null,
        component: null,
        storage_ids: ["stor1"],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const DeleteManyDocuments: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_documents",
      metadata: {
        component_id: null,
        component: null,
        table: "users",
        document_ids: Array.from(
          { length: 50 },
          (_, i) =>
            `jd72k9x8nmgz5h3qawc4btep0n6y${i.toString().padStart(4, "0")}`,
        ),
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const GenerateUploadUrl: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "generate_upload_url",
      metadata: {
        component_id: null,
        component: null,
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithComponents: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config_with_components",
      metadata: {
        auth_diff: { added: [], removed: [] },
        component_diffs: [
          {
            component_path: null,
            component_diff: {
              diffType: { type: "modify" },
              moduleDiff: { added: ["convex/myFunction.js"], removed: [] },
              cronDiff: { added: [], updated: [], deleted: [] },
              schemaDiff: null,
              udfConfigDiff: null,
            },
          },
        ],
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithComponentsAndMessage: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config_with_components",
      metadata: {
        auth_diff: { added: [], removed: [] },
        component_diffs: [
          {
            component_path: null,
            component_diff: {
              diffType: { type: "modify" },
              moduleDiff: { added: ["convex/myFunction.js"], removed: [] },
              cronDiff: { added: [], updated: [], deleted: [] },
              schemaDiff: null,
              udfConfigDiff: null,
            },
          },
        ],
        message: "Deploy new feature: add myFunction",
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithNodeVersionFirstSet: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config_with_components",
      metadata: {
        auth_diff: { added: [], removed: [] },
        component_diffs: [
          {
            component_path: null,
            component_diff: {
              diffType: { type: "modify" },
              moduleDiff: { added: [], removed: [] },
              cronDiff: { added: [], updated: [], deleted: [] },
              schemaDiff: null,
              udfConfigDiff: null,
            },
          },
        ],
        node_version_diff: {
          previous_version: null,
          next_version: "22",
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithNodeVersionUpdate: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config_with_components",
      metadata: {
        auth_diff: { added: [], removed: [] },
        component_diffs: [
          {
            component_path: null,
            component_diff: {
              diffType: { type: "modify" },
              moduleDiff: { added: [], removed: [] },
              cronDiff: { added: [], updated: [], deleted: [] },
              schemaDiff: null,
              udfConfigDiff: null,
            },
          },
        ],
        node_version_diff: {
          previous_version: "22",
          next_version: "20",
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const PushConfigWithNodeVersionUnset: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "push_config_with_components",
      metadata: {
        auth_diff: { added: [], removed: [] },
        component_diffs: [
          {
            component_path: null,
            component_diff: {
              diffType: { type: "modify" },
              moduleDiff: { added: [], removed: [] },
              cronDiff: { added: [], updated: [], deleted: [] },
              schemaDiff: null,
              udfConfigDiff: null,
            },
          },
        ],
        node_version_diff: {
          previous_version: "22",
          next_version: null,
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const CreateUsageLimit: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "create_usage_limit",
      metadata: {
        id: "ul_1",
        config: {
          name: null,
          metric: "functionCalls",
          window: "month",
          limitType: "disable",
          limit: BigInt(1_000_000),
          enabled: true,
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const UpdateUsageLimit: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "update_usage_limit",
      metadata: {
        id: "ul_1",
        previous: {
          name: null,
          metric: "functionCalls",
          window: "month",
          limitType: "disable",
          limit: BigInt(1_000_000),
          enabled: true,
        },
        current: {
          name: null,
          metric: "functionCalls",
          window: "month",
          limitType: "disable",
          limit: BigInt(2_000_000),
          enabled: false,
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

// A no-op save (neither the limit nor enabled changed) must render only the
// sentence, with no empty bordered box below it.
export const UpdateUsageLimitNoChange: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "update_usage_limit",
      metadata: {
        id: "ul_1",
        previous: {
          name: null,
          metric: "functionCalls",
          window: "month",
          limitType: "disable",
          limit: BigInt(1_000_000),
          enabled: true,
        },
        current: {
          name: null,
          metric: "functionCalls",
          window: "month",
          limitType: "disable",
          limit: BigInt(1_000_000),
          enabled: true,
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};

export const UsageLimitExceeded: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "usage_limit_exceeded",
      metadata: {
        id: "ul_2",
        config: {
          name: null,
          metric: "databaseIoGb",
          window: "day",
          limitType: "warning",
          limit: BigInt(50),
          enabled: true,
        },
      },
      memberName: "System",
      member_id: null,
      token_id: null,
      app_client_id: null,
    },
  },
};

export const DeleteUsageLimit: Story = {
  args: {
    event: {
      _id: "" as Id<"_deployment_audit_log">,
      _creationTime: Date.parse("12/19/2022, 10:00:00 AM"),
      action: "delete_usage_limit",
      metadata: {
        id: "ul_2",
        config: {
          name: null,
          metric: "databaseIoGb",
          window: "day",
          limitType: "warning",
          limit: BigInt(50),
          enabled: false,
        },
      },
      memberName: "member@convex.dev",
      member_id: BigInt(1),
      token_id: BigInt(1),
      app_client_id: null,
    },
  },
};
