import { Meta, StoryObj } from "@storybook/nextjs";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { ProjectDetails } from "generatedApi";
import {
  AggregatedFunctionMetrics,
  REST_OF_FUNCTIONS,
} from "hooks/usageMetrics";
import { rootComponentPath } from "api/usage";
import { Sheet } from "@ui/Sheet";
import {
  FunctionBreakdownMetric,
  FunctionBreakdownMetricAiGateway,
  FunctionBreakdownMetricCalls,
  FunctionBreakdownMetricCompute,
  FunctionBreakdownMetricDatabaseIO,
  FunctionBreakdownMetricDataEgress,
  FunctionBreakdownMetricSearch,
  TeamUsageByFunctionChart,
} from "./TeamUsageByFunctionChart";

const meta = {
  component: TeamUsageByFunctionChart,
  render: (args) => (
    <Sheet>
      <TeamUsageByFunctionChart {...args} />
    </Sheet>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof TeamUsageByFunctionChart>;

export default meta;
type Story = StoryObj<typeof meta>;

const team = {
  id: 42,
  name: "My amazing team",
  creator: 1,
  slug: "my-amazing-team",
  suspended: false,
  referralCode: "MYAMAZ1341",
  referredBy: null,
};

const project: ProjectDetails = {
  id: 42,
  teamId: 42,
  slug: "my-project",
  name: "My Project",
  createTime: 0,
};

const deployments: PlatformDeploymentResponse[] = [
  {
    kind: "cloud",
    id: 10,
    projectId: 42,
    name: "fabulous-goldfish-42",
    deploymentUrl: "https://fabulous-goldfish-42.convex.cloud",
    deploymentType: "dev",
    createTime: 0,
    creator: 1,
    previewIdentifier: null,
    region: "aws-us-east-1",
    isDefault: true,
    reference: "dev/nicolas",
    class: "s16",
  },
  {
    kind: "cloud",
    id: 20,
    projectId: 42,
    name: "friendly-dog-64",
    deploymentUrl: "https://friendly-dog-64.convex.cloud",
    deploymentType: "dev",
    createTime: 0,
    creator: 2,
    previewIdentifier: null,
    region: "aws-us-east-1",
    isDefault: true,
    reference: "dev/ari",
    class: "s16",
  },
  {
    kind: "cloud",
    id: 30,
    projectId: 42,
    name: "wandering-fish-513",
    deploymentUrl: "https://wandering-fish-513.convex.cloud",
    deploymentType: "prod",
    createTime: 0,
    creator: 1,
    previewIdentifier: null,
    region: "aws-us-east-1",
    isDefault: true,
    reference: "production",
    class: "s16",
  },
  {
    kind: "cloud",
    id: 40,
    projectId: 42,
    name: "happy-capybara-123",
    deploymentUrl: "https://happy-capybara-123.convex.cloud",
    deploymentType: "custom",
    createTime: 0,
    creator: 1,
    previewIdentifier: null,
    region: "aws-us-east-1",
    isDefault: false,
    reference: "staging",
    class: "s16",
  },
];

const rows: AggregatedFunctionMetrics[] = [
  {
    projectId: 42,
    deploymentType: "prod",
    function: "module.js:getLeague",
    callCount: 1,
    databaseIngressSize: 349567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "prod",
    function: "folder/module.js:setProfile",
    callCount: 10_000,
    databaseIngressSize: 67,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "prod",
    function: "module.js:thisFunctionHasAReallyLongName",
    callCount: 2_000,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "custom",
    function: "module.js:thisFunctionHasAReallyLongName",
    callCount: 23_456,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 1234,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 5678,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "prod",
    function: "module.js:setAvatar",
    callCount: 9_000,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "dev",
    function: "module.js:getBalls",
    callCount: 1_000,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "dev",
    function: "/api/endpoint",
    callCount: 12345,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 98765,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "dev",
    function: "devMerge.js:example",
    callCount: 40,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "prod",
    function: "folder/module.js:setScore",
    callCount: 16_000,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "prod",
    function: "module.js:default",
    callCount: 21_034,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 20,
    vectorSearchGb: 10,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "dev",
    function: "devMerge.js:example",
    callCount: 2,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 10,
    vectorSearchGb: 50,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 2,
    deploymentType: "preview",
    function: "former_project_function.js:default",
    callCount: 22_022,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "prod",
    function: "module.js:getBalls",
    callCount: 25_431,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0,
    actionComputeConvexTime: 0,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "prod",
    function: "module.js:getBall",
    callCount: 30_000,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0.0012,
    actionComputeConvexTime: 0.0011,
    actionComputeNodeTime: 0,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "prod",
    function: "folder/module.js:getScore",
    callCount: 20_000,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 0.1,
    actionComputeConvexTime: 0.02,
    actionComputeNodeTime: 0.003,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  {
    projectId: 42,
    deploymentType: "dev",
    function: "folder/module.js:getScore",
    callCount: 200,
    databaseIngressSize: 34567,
    databaseEgressSize: 12345,
    textSearchGb: 0,
    vectorSearchGb: 0,
    queryMutationComputeTime: 12.4,
    actionComputeConvexTime: 10.0,
    actionComputeNodeTime: 10.0456778,
    dataEgress: 0,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
  // The roll-up of the functions below the query's per-project cut-off. It is
  // the tail of a skewed distribution, so in practice it lands well under the
  // busiest individual function.
  {
    projectId: 42,
    deploymentType: null,
    function: REST_OF_FUNCTIONS,
    callCount: 4_800,
    databaseIngressSize: 12_000,
    databaseEgressSize: 9_500,
    textSearchGb: 4,
    vectorSearchGb: 11,
    queryMutationComputeTime: 1.4,
    actionComputeConvexTime: 0.9,
    actionComputeNodeTime: 0.7,
    dataEgress: 21_000,
    aiGatewayCost: 0,
    componentPath: rootComponentPath,
  },
];

const maxValueFor = (
  metric: FunctionBreakdownMetric,
  forRows: AggregatedFunctionMetrics[] = rows,
) => Math.max(...forRows.map(metric.getTotal));

// Search usage can live entirely in functions below the query's per-project
// cut-off, leaving the roll-up as the only row with a value.
const rowsWithSearchOnlyInRest: AggregatedFunctionMetrics[] = rows.map((row) =>
  row.function === REST_OF_FUNCTIONS
    ? row
    : { ...row, textSearchGb: 0, vectorSearchGb: 0 },
);

export const Default: Story = {
  args: {
    rows,
    team,
    project,
    deployments,
    maxValue: maxValueFor(FunctionBreakdownMetricCalls),
    metric: FunctionBreakdownMetricCalls,
  },
};

export const LocalAiSpend: Story = {
  args: {
    rows: (["local", "local", "dev", "prod"] as const).map(
      (deploymentType) => ({
        ...rows[0],
        function: "summaries.js:summarize",
        deploymentType,
        aiGatewayCost: 0.25,
      }),
    ),
    team,
    project,
    deployments,
    maxValue: 0.5,
    metric: FunctionBreakdownMetricAiGateway,
  },
};

export const ForDeletedProject: Story = {
  args: {
    rows,
    team,
    project: null,
    deployments: [],
    maxValue: maxValueFor(FunctionBreakdownMetricCalls),
    metric: FunctionBreakdownMetricCalls,
  },
};

export const DatabaseIO: Story = {
  args: {
    rows,
    team,
    project,
    deployments,
    maxValue: maxValueFor(FunctionBreakdownMetricDatabaseIO),
    metric: FunctionBreakdownMetricDatabaseIO,
  },
};

export const Compute: Story = {
  args: {
    rows,
    team,
    project,
    deployments,
    maxValue: maxValueFor(FunctionBreakdownMetricCompute),
    metric: FunctionBreakdownMetricCompute,
  },
};

export const Search: Story = {
  args: {
    rows,
    team,
    project,
    deployments,
    maxValue: maxValueFor(FunctionBreakdownMetricSearch),
    metric: FunctionBreakdownMetricSearch,
  },
};

export const DataEgress: Story = {
  args: {
    rows,
    team,
    project,
    deployments,
    maxValue: maxValueFor(FunctionBreakdownMetricDataEgress),
    metric: FunctionBreakdownMetricDataEgress,
  },
};

export const OnlyRestHasUsage: Story = {
  args: {
    rows: rowsWithSearchOnlyInRest,
    team,
    project,
    deployments,
    maxValue: maxValueFor(
      FunctionBreakdownMetricSearch,
      rowsWithSearchOnlyInRest,
    ),
    metric: FunctionBreakdownMetricSearch,
  },
};
