import { Checkbox } from "@ui/Checkbox";
import isEqual from "lodash/isEqual";
import { S3ExportTableSelector } from "./S3ExportTableSelector";
import { Selection } from "@convex-dev/platform/deploymentApi";
import {
  componentTablesIncluded,
  selectionWithComponents,
} from "./s3ExportSelection";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { TextInput } from "@ui/TextInput";
import { Combobox, Option } from "@ui/Combobox";
import { Stepper } from "@ui/Stepper";
import { SegmentedControl } from "@ui/SegmentedControl";
import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import {
  ChevronDownIcon,
  ChevronUpIcon,
  EyeNoneIcon,
  EyeOpenIcon,
  ExternalLinkIcon,
} from "@radix-ui/react-icons";
import { useFormik } from "formik";
import { useContext, useMemo, useState } from "react";
import { Callout } from "@ui/Callout";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import * as Yup from "yup";
import {
  SyncPeriod,
  SyncSelection,
} from "system-udfs/convex/_system/frontend/common";
import { AnalyticsIntegration } from "@common/lib/integrationHelpers";
import {
  useCreateLogStream,
  useDeleteLogStream,
  useUpdateLogStream,
} from "@common/lib/integrationsApi";
import { toast } from "@common/lib/utils";
import { CopyButton } from "@common/elements/CopyButton";
import { SyncPeriodSelector } from "./SyncPeriodSelector";
import {
  exportRoot,
  glueDatabaseName,
  iamPolicy,
  useDeploymentName,
} from "./S3ExportStatus";

// Mirrors `validate_s3_bucket` in crates/local_backend/src/log_sinks.rs.
// https://docs.aws.amazon.com/AmazonS3/latest/userguide/bucketnamingrules.html
const bucketSchema = Yup.string()
  .required("Bucket name is required")
  .min(3, "Bucket names are between 3 and 63 characters")
  .max(63, "Bucket names are between 3 and 63 characters")
  .matches(
    /^[a-z0-9][a-z0-9.-]*[a-z0-9]$/,
    "Bucket names may only use lowercase letters, numbers, dots, and hyphens, and must start and end with a letter or number",
  )
  .test(
    "no-adjacent-periods",
    "Bucket names may not contain two adjacent periods",
    (value) => !value?.includes(".."),
  )
  .test(
    "not-ip-address",
    "Bucket names may not be formatted as an IP address",
    (value) => !/^\d{1,3}(\.\d{1,3}){3}$/.test(value ?? ""),
  )
  .test(
    "no-reserved-affix",
    "Bucket names may not start with `xn--` or `sthree-`, or end with `-s3alias` or `--ol-s3`",
    (value) =>
      !["xn--", "sthree-"].some((p) => value?.startsWith(p)) &&
      !["-s3alias", "--ol-s3"].some((suffix) => value?.endsWith(suffix)),
  );

// The fields each step collects, in step order. Leaving a new role export's
// first step creates it, since Connect AWS needs the External ID the backend
// generates.
const ROLE_STEP_FIELDS = [
  ["bucket", "region", "prefix"],
  ["roleArn"],
  ["period"],
] as const;
// Access-key exports keep the shape they were created with.
const ACCESS_KEY_STEP_FIELDS = [
  ["bucket", "region", "prefix"],
  ["accessKeyId", "secretAccessKey"],
  ["period"],
] as const;

/**
 * The same role the CloudFormation template creates, as Terraform for teams
 * that keep AWS in their own repo. Mirrors
 * ops/services/cloudformation/convex-integrations.yaml; change both together.
 */
export function terraformRole({
  externalId,
  bucket,
  exportPath,
  glueDatabase,
  createBucket = false,
}: {
  externalId: string;
  bucket: string;
  exportPath: string;
  glueDatabase: string;
  /** Matches the template's `CreateBucket`: the snippet makes the bucket too. */
  createBucket?: boolean;
}) {
  const convexAccount = CONVEX_S3_EXPORT_ROLE_ARN.split(":")[4];
  const bucketResources = createBucket
    ? `# Bucket for the export. Keep it if you remove Convex: it holds your data.
resource "aws_s3_bucket" "convex_export" {
  bucket = "${bucket}"
}

resource "aws_s3_bucket_public_access_block" "convex_export" {
  bucket                  = aws_s3_bucket.convex_export.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_server_side_encryption_configuration" "convex_export" {
  bucket = aws_s3_bucket.convex_export.id
  rule {
    apply_server_side_encryption_by_default { sse_algorithm = "AES256" }
  }
}

resource "aws_s3_bucket_lifecycle_configuration" "convex_export" {
  bucket = aws_s3_bucket.convex_export.id
  rule {
    id     = "abort-incomplete-uploads"
    status = "Enabled"
    filter {}
    abort_incomplete_multipart_upload { days_after_initiation = 7 }
  }
}

`
    : "";
  return `${bucketResources}# Role Convex assumes to write this deployment's data lake export.
resource "aws_iam_role" "convex_export" {
  name = "ConvexIntegrations-${externalId}"
  path = "/convex/"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { AWS = "arn:aws:iam::${convexAccount}:root" }
      Condition = {
        StringEquals = { "sts:ExternalId" = "${externalId}" }
        ArnEquals    = { "aws:PrincipalArn" = "${CONVEX_S3_EXPORT_ROLE_ARN}" }
      }
    }]
  })
}

resource "aws_iam_role_policy" "convex_export" {
  role = aws_iam_role.convex_export.id
  name = "data-lake"
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect   = "Allow"
        Action   = "s3:GetBucketLocation"
        Resource = "arn:aws:s3:::${bucket}"
      },
      {
        Effect    = "Allow"
        Action    = "s3:ListBucket"
        Resource  = "arn:aws:s3:::${bucket}"
        Condition = { StringLike = { "s3:prefix" = ["${exportPath}/*", "${exportPath}"] } }
      },
      {
        Effect   = "Allow"
        Action   = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload"]
        Resource = "arn:aws:s3:::${bucket}/${exportPath}/*"
      },
      {
        Effect = "Allow"
        Action = [
          "glue:GetDatabase", "glue:CreateDatabase", "glue:GetTable", "glue:GetTables",
          "glue:CreateTable", "glue:UpdateTable", "glue:DeleteTable",
        ]
        Resource = [
          "arn:aws:glue:\${data.aws_region.current.name}:\${data.aws_caller_identity.current.account_id}:catalog",
          "arn:aws:glue:\${data.aws_region.current.name}:\${data.aws_caller_identity.current.account_id}:database/${glueDatabase}",
          "arn:aws:glue:\${data.aws_region.current.name}:\${data.aws_caller_identity.current.account_id}:table/${glueDatabase}/*",
        ]
      },
    ]
  })
}

data "aws_region" "current" {}
data "aws_caller_identity" "current" {}

# Paste this, or your AWS account ID, into the Convex dashboard.
output "convex_export_role_arn" {
  value = aws_iam_role.convex_export.arn
}
`;
}

// Where the dashboard sends the admin to create the customer-side role. Both
// values come from ops/services/conductor/terraform and ops/vault/porter.tf.
const CLOUDFORMATION_TEMPLATE_URL =
  "https://convex-cloudformation-use1-prod.s3.us-east-1.amazonaws.com/convex-integrations/template.yaml";
export const CONVEX_S3_EXPORT_ROLE_ARN =
  "arn:aws:iam::715951115348:role/convex-s3-export-prod";

/**
 * CloudFormation quick-create link with every parameter filled in, so the
 * admin only reviews and clicks Create stack.
 */
export function quickCreateUrl({
  region,
  externalId,
  bucket,
  exportPath,
  glueDatabase,
  createBucket = false,
}: {
  region: string;
  externalId: string;
  bucket: string;
  /** `exportRoot(prefix, deploymentName)/data-lake`, the one path the role may touch. */
  exportPath: string;
  glueDatabase: string;
  /** The stack creates `bucket` in `region` instead of using an existing one. */
  createBucket?: boolean;
}) {
  const params = new URLSearchParams({
    templateURL: CLOUDFORMATION_TEMPLATE_URL,
    stackName: `ConvexIntegrations-${externalId.slice(0, 8)}`,
    param_ExternalId: externalId,
    param_BucketName: bucket,
    param_ExportPath: exportPath,
    param_ConvexRoleArn: CONVEX_S3_EXPORT_ROLE_ARN,
    param_GlueDatabase: glueDatabase,
    param_CreateBucket: String(createBucket),
  });
  return `https://${region}.console.aws.amazon.com/cloudformation/home?region=${region}#/stacks/quickcreate?${params}`;
}

/**
 * The role an export's stack creates is named after its External ID, so the
 * customer's AWS account ID is the only part Convex doesn't know. Accepts the
 * account ID, or any ARN from the customer's account (the stack's ARN, or
 * `RoleArn` itself), and returns `undefined` for anything else. Porter may
 * only assume `role/convex/ConvexIntegrations-*`, so no other role name works.
 */
export function roleArnFor(input: string, externalId: string) {
  const value = input.trim();
  const roleArn = (account: string) =>
    `arn:aws:iam::${account}:role/convex/ConvexIntegrations-${externalId}`;
  // The AWS account menu shows IDs as 1234-5678-9012.
  const digits = value.replace(/[\s-]/g, "");
  if (/^\d{12}$/.test(digits)) {
    return roleArn(digits);
  }
  const arn = value.match(/^arn:aws:([a-z0-9-]+):[a-z0-9-]*:(\d{12}):(.*)$/);
  if (!arn) {
    return undefined;
  }
  const [, service, account, resource] = arn;
  if (
    service === "iam" &&
    resource.startsWith("role/") &&
    value !== roleArn(account)
  ) {
    return undefined;
  }
  return roleArn(account);
}

export function S3ExportConfigurationForm({
  onClose,
  integration,
  onAddedIntegration,
}: {
  onClose: () => void;
  integration: Extract<AnalyticsIntegration, { kind: "s3Export" }>;
  onAddedIntegration?: () => void;
}) {
  const createLogStream = useCreateLogStream();
  const updateLogStream = useUpdateLogStream();
  const deleteLogStream = useDeleteLogStream();
  const deploymentInfo = useContext(DeploymentInfoContext);
  // Deployment regions are named "aws-us-east-1"; S3 regions "us-east-1".
  const deploymentRegion = ((d) => (d && "region" in d ? d.region : ""))(
    deploymentInfo.useCurrentDeployment(),
  ).replace(/^aws-/, "");
  const existingConfig = integration.existing?.config ?? null;
  const logStreamId = integration.existing?._id;

  const initialSelection: SyncSelection = {
    _other: "included",
    ...existingConfig?.selection,
    "": existingConfig?.selection[""] ?? {
      _other:
        existingConfig?.selection._other === "excluded"
          ? "excluded"
          : "included",
    },
  };

  const isNewIntegration = existingConfig === null || !logStreamId;

  const [showSecretAccessKey, setShowSecretAccessKey] = useState(false);
  // New exports use a role. A stored role export carries an empty access key
  // ID, so the role fields decide which kind a saved export is.
  const usesAccessKeys =
    existingConfig !== null && existingConfig.roleExternalId === undefined;
  const stepFields = usesAccessKeys ? ACCESS_KEY_STEP_FIELDS : ROLE_STEP_FIELDS;
  const awaitingRole = !usesAccessKeys && existingConfig?.roleArn === "";
  // The worker records why AWS refused the export, e.g. a role it cannot
  // assume. For a role export that is almost always fixed on Connect AWS.
  const externalId = existingConfig?.roleExternalId ?? "";
  const failure =
    !usesAccessKeys && integration.existing?.status.type === "failed"
      ? integration.existing.status.reason
      : undefined;
  const deploymentName = useDeploymentName();
  // Whether the stack creates the bucket. Follows the bucket choice while
  // adding; on a reopened export the choice is gone, so it starts on: a
  // stack aimed at an existing bucket fails loudly in CloudFormation, while a
  // missing bucket would only surface at the first sync.
  const [createBucket, setCreateBucket] = useState(true);
  // Set once this form creates the export, so Connect AWS can tell the bucket
  // choice from step 1 is still known.
  const [createdHere, setCreatedHere] = useState(false);
  const [confirmFinishLater, setConfirmFinishLater] = useState(false);

  const validationSchema = useMemo(
    () =>
      Yup.object().shape({
        bucket: bucketSchema,
        region: Yup.string().required("Region is required"),
        // IAM reads `*` and `?` as wildcards, which would widen the generated
        // policy beyond the export's directory.
        // Mirrors `validate_s3_prefix` in crates/local_backend/src/log_sinks.rs.
        prefix: Yup.string().matches(
          /^[^*?$]*$/,
          "Prefixes may not contain `*`, `?` or `$`",
        ),
        accessKeyId: usesAccessKeys
          ? Yup.string().required("Access key ID is required")
          : Yup.string(),
        secretAccessKey:
          usesAccessKeys && isNewIntegration
            ? Yup.string().required("Secret access key is required")
            : // Blank keeps the stored secret.
              Yup.string(),
        roleArn: (!usesAccessKeys && !isNewIntegration
          ? Yup.string().required(
              "Paste your AWS account ID or the stack's ARN",
            )
          : Yup.string()
        )
          .test(
            "partition",
            "S3 export supports commercial AWS regions only",
            (value) => !value || !/^arn:aws-(cn|us-gov):/.test(value.trim()),
          )
          .test(
            "role",
            `Convex can only use the role convex/ConvexIntegrations-${externalId}. Paste your AWS account ID instead.`,
            (value) =>
              !value ||
              !/^arn:aws:iam::\d{12}:role\//.test(value.trim()) ||
              roleArnFor(value, externalId) !== undefined,
          )
          .test(
            "account",
            "Paste a 12-digit AWS account ID, or an ARN from that account",
            (value) => !value || roleArnFor(value, externalId) !== undefined,
          ),
      }),
    [externalId, isNewIntegration, usesAccessKeys],
  );

  const formState = useFormik<{
    bucket: string;
    region: string;
    prefix: string;
    accessKeyId: string;
    secretAccessKey: string;
    roleArn: string;
    period: SyncPeriod;
    selection: SyncSelection;
  }>({
    initialValues: {
      bucket:
        existingConfig?.bucket ??
        // Bucket names are global, so the suggestion carries the deployment
        // name, which is unique.
        `convex-export-${deploymentName}`,
      region: existingConfig?.region ?? deploymentRegion,
      prefix: existingConfig?.prefix ?? "",
      accessKeyId: existingConfig?.accessKeyId ?? "",
      secretAccessKey: "",
      // Saved role ARNs differ only in the account, so show just that.
      roleArn: existingConfig?.roleArn?.split(":")[4] ?? "",
      period: existingConfig?.period ?? "daily",
      selection: initialSelection,
    },
    onSubmit: async (values, helpers) => {
      helpers.setStatus(undefined);
      try {
        const selection = isEqual(values.selection, initialSelection)
          ? undefined
          : values.selection;
        const config = {
          bucket: values.bucket,
          region: values.region,
          prefix: values.prefix || null,
          period: values.period,
          // OpenAPI's flattened map type cannot represent the `_other` default.
          ...(selection ? { selection: selection as Selection } : {}),
        };

        if (isNewIntegration) {
          await createLogStream({
            logStreamType: "s3Export",
            ...config,
            accessKeyId: values.accessKeyId,
            secretAccessKey: values.secretAccessKey,
          });
          onAddedIntegration?.();
          toast("success", "Export saved. The first sync is starting.");
        } else {
          // Omitting the secret keeps the one already stored; omitting the
          // ARN keeps the one already pasted.
          const roleArn = roleArnFor(values.roleArn, externalId) ?? "";
          await updateLogStream(logStreamId, {
            logStreamType: "s3Export",
            period: values.period,
            ...(selection ? { selection: selection as Selection } : {}),
            ...(usesAccessKeys && values.secretAccessKey
              ? { secretAccessKey: values.secretAccessKey }
              : {}),
            ...(!usesAccessKeys && roleArn !== existingConfig.roleArn
              ? { roleArn }
              : {}),
          });
          toast(
            "success",
            !usesAccessKeys &&
              existingConfig.roleArn === "" &&
              values.roleArn !== ""
              ? "Role saved. The first sync is starting."
              : "Export updated.",
          );
        }
        onClose();
      } catch (e) {
        helpers.setStatus({
          error: e instanceof Error ? e.message : "Failed to save integration.",
        });
      }
    },
    validationSchema,
  });

  // The export is created without its ARN so the backend can hand back the
  // External ID the stack needs. Nothing syncs until the ARN is saved.
  const [creating, setCreating] = useState(false);
  const createRoleExport = async () => {
    const { bucket, region, prefix } = formState.values;
    formState.setStatus(undefined);
    setCreating(true);
    try {
      await createLogStream({
        logStreamType: "s3Export",
        bucket,
        region,
        prefix: prefix || null,
        period: formState.values.period,
        roleArn: "",
      });
      onAddedIntegration?.();
      setCreatedHere(true);
      setActiveStep(1);
    } catch (e) {
      formState.setStatus({
        error: e instanceof Error ? e.message : "Failed to create the export.",
      });
    } finally {
      setCreating(false);
    }
  };

  // Formik validates every field on each change; show a field's error only
  // once the user has left it or tried to save.
  const fieldError = (
    field: Exclude<keyof typeof formState.values, "selection">,
  ) =>
    formState.touched[field] || formState.submitCount > 0
      ? formState.errors[field]
      : undefined;
  const { bucket, region, prefix } = formState.values;
  const policy = iamPolicy({ bucket, region, prefix, deploymentName });
  // The first invalid field in a step, if any.
  const stepError = (step: number) =>
    stepFields[step]
      .map((field) => {
        try {
          validationSchema.validateSyncAt(field, formState.values);
          return undefined;
        } catch (e) {
          return e instanceof Yup.ValidationError ? e.message : String(e);
        }
      })
      .find((error) => error !== undefined);
  // The policy can't be built without a valid destination.
  const destinationError = stepError(0);
  // A new export walks through the steps in order. A saved one opens where
  // the admin is most likely headed: Connect AWS while the role is missing,
  // otherwise its sync settings.
  const [activeStep, setActiveStep] = useState(
    isNewIntegration ? 0 : awaitingRole || failure !== undefined ? 1 : 2,
  );
  const continueButton = (
    step: number,
    label = "Continue",
    then = async () => setActiveStep(step + 1),
  ) => (
    <Button
      variant="neutral"
      size="sm"
      className="self-start"
      loading={creating}
      onClick={async () => {
        await formState.setTouched(
          {
            ...formState.touched,
            ...Object.fromEntries(stepFields[step].map((f) => [f, true])),
          },
          true,
        );
        if (stepError(step) === undefined) {
          await then();
        }
      }}
    >
      {label}
    </Button>
  );
  // Until the role is connected, the steps run in order and only the last one
  // saves, so the ARN reaches the backend together with the sync settings.
  const pending = !usesAccessKeys && (isNewIntegration || awaitingRole);
  const resolvedRoleArn = roleArnFor(formState.values.roleArn, externalId);
  const roleArnChanged =
    (resolvedRoleArn ?? formState.values.roleArn) !==
    (existingConfig?.roleArn ?? "");
  // Match the destination fields enforced by `ensure_same_s3_destination`.
  const destinationFixed = !isNewIntegration;
  const destinationFixedDescription =
    "Fixed for this export. Delete it and create a new one to change this.";

  const destinationFields = (
    <>
      <TextInput
        value={bucket}
        onChange={formState.handleChange}
        label="Bucket name"
        placeholder="my-analytics-bucket"
        id="bucket"
        error={fieldError("bucket")}
        onBlur={formState.handleBlur}
        disabled={destinationFixed}
        description={
          destinationFixed
            ? destinationFixedDescription
            : createBucket
              ? "The CloudFormation stack creates this bucket, private and encrypted. Names are unique across all of AWS."
              : "A bucket that already exists in your AWS account."
        }
      />
      <div className="flex flex-col gap-1">
        <Combobox
          label="Region"
          labelHidden={false}
          options={AWS_REGIONS}
          selectedOption={region || null}
          setSelectedOption={async (value) => {
            await formState.setFieldValue("region", value ?? "");
            await formState.setFieldTouched("region", true, false);
          }}
          placeholder="Select the bucket's region"
          disabled={destinationFixed}
          allowCustomValue
          unknownLabel={(value) => value}
          buttonClasses="w-full bg-inherit"
        />
        {fieldError("region") && (
          <p className="text-xs text-content-errorSecondary" role="alert">
            {fieldError("region")}
          </p>
        )}
      </div>
      <TextInput
        value={prefix}
        onChange={formState.handleChange}
        label="Prefix (optional)"
        placeholder="convex/"
        id="prefix"
        error={fieldError("prefix")}
        onBlur={formState.handleBlur}
        disabled={destinationFixed}
      />
      {!destinationError && (
        <p className="text-xs text-content-secondary">
          Convex writes Apache Iceberg tables to{" "}
          <code>
            s3://{bucket}/{exportRoot(prefix, deploymentName)}/data-lake/
          </code>{" "}
          and registers them in the AWS Glue database{" "}
          <code>{glueDatabaseName(deploymentName)}</code>, which Convex creates.
        </p>
      )}
    </>
  );

  return (
    <form
      onSubmit={formState.handleSubmit}
      className="flex min-h-0 flex-1 flex-col"
    >
      <div className="scrollbar flex min-h-0 flex-1 flex-col overflow-y-auto px-6 pb-4">
        <Stepper activeStep={activeStep} onSelectStep={setActiveStep}>
          {usesAccessKeys ? (
            <Stepper.Step label="Destination">
              <Callout variant="instructions">
                Access keys are being retired. To switch to an IAM role, delete
                this export and add it again with +. That needs a new AWS stack,
                re-exports every table, and the bucket gets no syncs until the
                role is connected.
              </Callout>
              {destinationFields}
              {continueButton(0)}
            </Stepper.Step>
          ) : (
            <Stepper.Step label="Bucket">
              {isNewIntegration && (
                <fieldset className="flex flex-col gap-2">
                  <legend className="sr-only">Bucket</legend>
                  {[
                    [true, "Create a new bucket"],
                    [false, "Use an existing bucket"],
                  ].map(([value, label]) => (
                    <label
                      key={String(value)}
                      className="flex items-center gap-2 text-sm"
                    >
                      <input
                        type="radio"
                        name="bucketChoice"
                        checked={createBucket === value}
                        onChange={() => {
                          setCreateBucket(value as boolean);
                          void formState.setFieldValue(
                            "bucket",
                            value ? `convex-export-${deploymentName}` : "",
                          );
                        }}
                      />
                      {label}
                    </label>
                  ))}
                </fieldset>
              )}
              {destinationFields}
              {isNewIntegration ? (
                <div className="flex flex-col gap-1">
                  {continueButton(0, "Continue", createRoleExport)}
                  <p className="text-xs text-content-secondary">
                    Creates the export. Nothing syncs until you finish.
                  </p>
                </div>
              ) : (
                continueButton(0)
              )}
            </Stepper.Step>
          )}
          {usesAccessKeys && (
            <Stepper.Step label="AWS access">
              <>
                <PolicyActions
                  policy={policy}
                  disabledReason={destinationError}
                >
                  <ol className="flex list-[lower-alpha] flex-col gap-1 pl-4 text-xs text-content-secondary">
                    <li>
                      In the bucket's AWS account,{" "}
                      <Link
                        href="https://console.aws.amazon.com/iam/home#/users/create"
                        target="_blank"
                      >
                        create an IAM user
                      </Link>
                      .
                    </li>
                    <li>
                      Open the user, then choose Add permissions, Create inline
                      policy, and JSON. Paste this policy and save it.
                    </li>
                    <li>
                      Under Security credentials, create an access key for an
                      application running outside AWS. Copy both values: AWS
                      shows the secret only once.
                    </li>
                  </ol>
                </PolicyActions>
                <TextInput
                  value={formState.values.accessKeyId}
                  onChange={formState.handleChange}
                  label="Access Key ID"
                  placeholder="AKIAIOSFODNN7EXAMPLE"
                  id="accessKeyId"
                  error={fieldError("accessKeyId")}
                  onBlur={formState.handleBlur}
                  disabled={destinationFixed}
                  description={
                    destinationFixed ? destinationFixedDescription : undefined
                  }
                />
                <TextInput
                  value={formState.values.secretAccessKey}
                  onChange={formState.handleChange}
                  type={showSecretAccessKey ? "text" : "password"}
                  label={
                    isNewIntegration
                      ? "Secret Access Key"
                      : "Secret Access Key (stored)"
                  }
                  id="secretAccessKey"
                  className="max-w-full"
                  placeholder={
                    isNewIntegration ? undefined : "Leave blank to keep"
                  }
                  action={() => setShowSecretAccessKey(!showSecretAccessKey)}
                  Icon={showSecretAccessKey ? EyeNoneIcon : EyeOpenIcon}
                  iconTooltip={
                    showSecretAccessKey
                      ? "Hide secret access key"
                      : "Show secret access key"
                  }
                  error={fieldError("secretAccessKey")}
                  onBlur={formState.handleBlur}
                  description="Permissions can take a minute to apply. Convex retries automatically."
                />
              </>
              {continueButton(1)}
            </Stepper.Step>
          )}
          {!usesAccessKeys && (
            <Stepper.Step label="Connect AWS">
              {isNewIntegration ? (
                <p className="text-xs text-content-secondary" role="status">
                  Creating the export…
                </p>
              ) : (
                <>
                  {!createdHere && (
                    <div className="flex flex-col gap-1">
                      <label className="flex items-center gap-2 text-sm">
                        <Checkbox
                          checked={createBucket}
                          onChange={() => setCreateBucket(!createBucket)}
                        />
                        Create the bucket <code>{bucket}</code>
                      </label>
                      <p className="pl-6 text-xs text-content-secondary">
                        Turn this off if the bucket already exists.
                      </p>
                    </div>
                  )}
                  <RoleSetup
                    role={{
                      externalId,
                      bucket,
                      exportPath: `${exportRoot(
                        prefix,
                        deploymentName,
                      )}/data-lake`,
                      glueDatabase: glueDatabaseName(deploymentName),
                      createBucket,
                    }}
                    region={region}
                  />
                  <TextInput
                    value={formState.values.roleArn}
                    onChange={formState.handleChange}
                    label="AWS account ID"
                    id="roleArn"
                    placeholder="123456789012"
                    error={fieldError("roleArn") ?? failure}
                    onBlur={formState.handleBlur}
                    description={
                      resolvedRoleArn ? (
                        <>
                          Convex connects to <code>{resolvedRoleArn}</code>
                        </>
                      ) : (
                        "Find it in the account menu at the top right of the AWS console. You can also paste the stack's ARN."
                      )
                    }
                  />
                  <div className="flex flex-col gap-1">
                    {continueButton(1, "Next: sync settings")}
                    {pending && (
                      <p className="text-xs text-content-secondary">
                        Nothing is saved until you save on the next step.
                      </p>
                    )}
                  </div>
                </>
              )}
            </Stepper.Step>
          )}
          <Stepper.Step label="Sync settings">
            <div className="overflow-hidden rounded-sm border">
              <div className="flex flex-col gap-1 p-3">
                <label
                  htmlFor="includeComponentTables"
                  className="flex items-center gap-2 text-sm"
                >
                  <Checkbox
                    id="includeComponentTables"
                    aria-label="Include component tables"
                    checked={componentTablesIncluded(
                      formState.values.selection,
                    )}
                    disabled={formState.isSubmitting}
                    onChange={() =>
                      void formState.setFieldValue(
                        "selection",
                        selectionWithComponents(
                          formState.values.selection,
                          componentTablesIncluded(
                            formState.values.selection,
                          ) !== true,
                        ),
                      )
                    }
                  />
                  Include component tables
                </label>
                <p className="pl-6 text-xs text-content-secondary">
                  Include data from components like aggregates and workpools.
                </p>
              </div>
              <Disclosure>
                <DisclosureButton className="flex w-full items-center justify-between gap-2 border-t px-3 py-2 text-left text-xs font-medium">
                  {({ open }) => (
                    <>
                      <span>Choose specific tables</span>
                      <span className="flex items-center gap-2 text-content-secondary">
                        Advanced
                        {open ? <ChevronUpIcon /> : <ChevronDownIcon />}
                      </span>
                    </>
                  )}
                </DisclosureButton>
                <DisclosurePanel className="border-t p-3">
                  <S3ExportTableSelector
                    selection={formState.values.selection}
                    onChange={(selection) =>
                      void formState.setFieldValue("selection", selection)
                    }
                    disabled={formState.isSubmitting}
                  />
                </DisclosurePanel>
              </Disclosure>
            </div>
            {!isNewIntegration &&
              !isEqual(formState.values.selection, initialSelection) && (
                <p className="text-xs text-content-secondary" role="status">
                  Saving keeps the progress of tables you already export. Tables
                  you add sync from the start, and tables you remove stay in S3
                  but stop updating.
                </p>
              )}
            <SyncPeriodSelector
              value={formState.values.period}
              onChange={async (period) => {
                await formState.setFieldValue("period", period);
              }}
            />
          </Stepper.Step>
        </Stepper>
      </div>
      <div className="flex items-center justify-end gap-2 px-6 py-4">
        {formState.status?.error && (
          <p className="text-sm text-content-errorSecondary" role="alert">
            {formState.status.error}
          </p>
        )}
        {awaitingRole && logStreamId && (
          <Button
            variant="danger"
            className="mr-auto"
            disabled={formState.isSubmitting}
            onClick={async () => {
              // A deployment holds one export, so a pending one left behind
              // blocks starting over.
              if (await deleteLogStream(logStreamId)) {
                toast("success", "Export discarded.");
                onClose();
              }
            }}
          >
            Discard export
          </Button>
        )}
        <Button
          variant="neutral"
          onClick={() =>
            awaitingRole && roleArnChanged
              ? setConfirmFinishLater(true)
              : onClose()
          }
          disabled={formState.isSubmitting}
        >
          {awaitingRole ? "Finish later" : "Cancel"}
        </Button>
        {confirmFinishLater && (
          <ConfirmationDialog
            onClose={() => setConfirmFinishLater(false)}
            onConfirm={async () => onClose()}
            confirmText="Finish later"
            dialogTitle="Leave without saving the role?"
            dialogBody="The account ID you pasted is not saved. The export stays pending until you save it."
          />
        )}
        {(!pending || activeStep === 2) && (
          <Button
            variant="primary"
            type="submit"
            aria-label="save"
            onClick={async () => {
              // Open the first step with an error, since the others are
              // collapsed.
              const errors = await formState.validateForm();
              const step = stepFields.findIndex((fields) =>
                fields.some((field) => errors[field]),
              );
              if (step !== -1) {
                setActiveStep(step);
              }
            }}
            // A new export starts fully filled in, so it can be saved untouched.
            disabled={
              (!isNewIntegration && !formState.dirty) || formState.isSubmitting
            }
            loading={formState.isSubmitting}
          >
            Save
          </Button>
        )}
      </div>
    </form>
  );
}

type SetupMethod = "console" | "terraform";

function RoleSetup({
  role,
  region,
}: {
  role: Parameters<typeof terraformRole>[0];
  region: string;
}) {
  const [method, setMethod] = useState<SetupMethod>("console");
  const terraform = terraformRole(role);
  return (
    <div className="flex flex-col gap-3">
      <SegmentedControl<SetupMethod>
        className="self-start"
        size="sm"
        options={[
          { label: "AWS console", value: "console" },
          { label: "Terraform", value: "terraform" },
        ]}
        value={method}
        onChange={setMethod}
      />
      {method === "console" ? (
        <ol className="flex list-decimal flex-col gap-3 pl-5 text-sm">
          <li>
            <div className="flex flex-col items-start gap-2">
              Open the stack in AWS. Every value is filled in.
              <Button
                href={quickCreateUrl({ region, ...role })}
                target="_blank"
                size="sm"
                icon={<ExternalLinkIcon />}
              >
                Open in AWS
              </Button>
            </div>
          </li>
          <li>
            Tick the IAM acknowledgement and choose Create stack. It takes one
            to two minutes.
          </li>
          <li>Paste your AWS account ID below.</li>
        </ol>
      ) : (
        <ol className="flex list-decimal flex-col gap-3 pl-5 text-sm">
          <li>
            <div className="flex flex-col gap-2">
              <div className="flex flex-col items-start gap-2">
                Add this to your Terraform. It creates the same role as the
                stack.
                <CopyButton text={terraform} label="Copy Terraform" size="xs" />
              </div>
              <pre className="scrollbar max-h-64 overflow-auto rounded-sm border bg-background-tertiary p-2 font-mono text-xs">
                {terraform}
              </pre>
            </div>
          </li>
          <li>
            Run <code>terraform apply</code>.
          </li>
          <li>Paste your AWS account ID below.</li>
        </ol>
      )}
    </div>
  );
}

function PolicyActions({
  policy,
  disabledReason,
  children,
}: {
  policy: string;
  disabledReason: string | undefined;
  children: React.ReactNode;
}) {
  return (
    <Disclosure as="div" className="flex flex-col gap-2">
      {children}
      <div className="flex flex-wrap items-center gap-2">
        <CopyButton
          text={policy}
          label="Copy policy"
          size="sm"
          disabled={disabledReason !== undefined}
        />
        <DisclosureButton as={Button} variant="neutral" size="sm">
          {({ open }) => <>{open ? "Hide policy" : "Show policy"}</>}
        </DisclosureButton>
        <p className="text-xs text-content-secondary" aria-live="polite">
          {disabledReason}
        </p>
      </div>
      <DisclosurePanel>
        <pre className="scrollbar max-h-64 overflow-auto rounded-sm border bg-background-tertiary p-2 font-mono text-xs">
          {policy}
        </pre>
      </DisclosurePanel>
    </Disclosure>
  );
}

// AWS regions with S3 and Glue. Other region codes can be typed in.
const AWS_REGIONS: Option<string>[] = [
  ["us-east-1", "US East (N. Virginia)"],
  ["us-east-2", "US East (Ohio)"],
  ["us-west-1", "US West (N. California)"],
  ["us-west-2", "US West (Oregon)"],
  ["af-south-1", "Africa (Cape Town)"],
  ["ap-east-1", "Asia Pacific (Hong Kong)"],
  ["ap-south-1", "Asia Pacific (Mumbai)"],
  ["ap-south-2", "Asia Pacific (Hyderabad)"],
  ["ap-southeast-1", "Asia Pacific (Singapore)"],
  ["ap-southeast-2", "Asia Pacific (Sydney)"],
  ["ap-southeast-3", "Asia Pacific (Jakarta)"],
  ["ap-southeast-4", "Asia Pacific (Melbourne)"],
  ["ap-northeast-1", "Asia Pacific (Tokyo)"],
  ["ap-northeast-2", "Asia Pacific (Seoul)"],
  ["ap-northeast-3", "Asia Pacific (Osaka)"],
  ["ca-central-1", "Canada (Central)"],
  ["ca-west-1", "Canada West (Calgary)"],
  ["eu-central-1", "Europe (Frankfurt)"],
  ["eu-central-2", "Europe (Zurich)"],
  ["eu-west-1", "Europe (Ireland)"],
  ["eu-west-2", "Europe (London)"],
  ["eu-west-3", "Europe (Paris)"],
  ["eu-south-1", "Europe (Milan)"],
  ["eu-south-2", "Europe (Spain)"],
  ["eu-north-1", "Europe (Stockholm)"],
  ["il-central-1", "Israel (Tel Aviv)"],
  ["me-south-1", "Middle East (Bahrain)"],
  ["me-central-1", "Middle East (UAE)"],
  ["sa-east-1", "South America (São Paulo)"],
].map(([value, name]) => ({ value, label: `${name} ${value}` }));
