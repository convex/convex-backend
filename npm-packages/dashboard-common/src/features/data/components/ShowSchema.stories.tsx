import { Meta, StoryObj } from "@storybook/nextjs";
import { Shape } from "shapes";
import { ShowSchema } from "@common/features/data/components/ShowSchema";
import { SchemaJson } from "@common/lib/format";
import { Sheet } from "@ui/Sheet";

const meta = {
  component: ShowSchema,
  args: {
    activeSchema: undefined,
    inProgressSchema: undefined,
    shapes: new Map<string, Shape>([
      [
        "tasks",
        {
          type: "Object",
          fields: [
            {
              fieldName: "status",
              optional: false,
              shape: { type: "String" },
            },
          ],
        },
      ],
    ]),
  },
  parameters: { a11y: { test: "todo" } },
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
} satisfies Meta<typeof ShowSchema>;

export default meta;
type Story = StoryObj<typeof meta>;

const sampleSchema: SchemaJson = {
  tables: [
    {
      tableName: "tasks",
      indexes: [],
      searchIndexes: [],
      vectorIndexes: [],
      documentType: {
        type: "object",
        value: {
          status: {
            fieldType: {
              type: "union",
              value: [
                { type: "literal", value: "todo" },
                { type: "literal", value: "in-progress" },
                { type: "literal", value: "done" },
              ],
            },
            optional: false,
          },
        },
      },
    },
  ],
  schemaValidation: true,
};

export const NoSchema: Story = { args: {} };

export const GenerationError: Story = {
  args: {
    hasShapeError: true,
  },
};

export const LoadingSchema: Story = {
  args: {
    inProgressSchema: sampleSchema,
  },
};

export const LoadingSchemaWithExistingSchema: Story = {
  args: {
    activeSchema: sampleSchema,
    inProgressSchema: sampleSchema,
  },
};

export const SavedSchema: Story = {
  args: {
    activeSchema: {
      ...sampleSchema,
      schemaValidation: false,
    },
  },
};

export const EnforcedSchema: Story = {
  args: {
    activeSchema: sampleSchema,
  },
};

export const SchemaValidationInProgress: Story = {
  args: {
    inProgressSchema: sampleSchema,
    schemaValidationProgress: {
      numDocsValidated: 1,
      totalDocs: 2,
    },
  },
};
