import {
  CheckCircledIcon,
  ExclamationTriangleIcon,
  PlusIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import classNames from "classnames";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { Combobox, Option } from "@ui/Combobox";
import { LocalDevCallout } from "@common/elements/LocalDevCallout";
import { Callout } from "@ui/Callout";
import { captureMessage } from "@sentry/nextjs";
import {
  ENVIRONMENT_VARIABLES_ROW_CLASSES,
  ENVIRONMENT_VARIABLE_NAME_COLUMN,
} from "@common/features/settings/components/EnvironmentVariables";
import { Sheet } from "@ui/Sheet";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { TextInput } from "@ui/TextInput";
import { useFormik } from "formik";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { useDeployments } from "api/deployments";
import { useCurrentProject } from "api/projects";
import { deploymentResource } from "lib/permissions";
import { useIsOperationAllowed } from "hooks/useDeploymentPermissions";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { Link } from "@ui/Link";
import { Fragment, useState, useMemo, ReactNode } from "react";
import {
  PlatformDeploymentResponse,
  PlatformCustomDomainResponse,
  PlatformDeleteCustomDomainArgs,
} from "@convex-dev/platform/managementApi";
import { TeamResponse, TeamEntitlementsResponse } from "generatedApi";
import {
  useListVanityDomains,
  useCreateVanityDomain,
  useDeleteVanityDomain,
} from "api/vanityDomains";
import { useDeploymentUrl } from "@common/lib/deploymentApi";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { useUpdateCanonicalUrl } from "hooks/deploymentApi";
import { Loading } from "@ui/Loading";
import { CopyButton } from "@common/elements/CopyButton";

const CLOUD_SUFFIX = ".convex.cloud";
const SITE_SUFFIX = ".convex.site";

// `*.<region>.convex.cloud` and `*.<region>.convex.site` are provisioned as a
// pair, so the site host is the cloud host with the suffix swapped.
export function defaultSiteUrl(deploymentUrl: string): string | null {
  const url = new URL(deploymentUrl);
  if (!url.hostname.endsWith(CLOUD_SUFFIX)) {
    return null;
  }
  url.hostname = `${url.hostname.slice(0, -CLOUD_SUFFIX.length)}${SITE_SUFFIX}`;
  return url.origin;
}

const VERIFICATION_DELAY_NOTE =
  "It may take up to 30 minutes to verify your domain and start serving traffic.";

function dnsRecords(vanityDomain: PlatformCustomDomainResponse) {
  return [
    { type: "CNAME", name: vanityDomain.domain, value: "convex.domains" },
    {
      type: "TXT",
      name: `_convex_domains.${vanityDomain.domain}`,
      value: vanityDomain.deploymentName,
    },
  ];
}

// Plain text so the instructions can be pasted into a chat, a ticket, or an
// agent prompt by someone who doesn't have access to the dashboard.
export function dnsInstructionsText(
  vanityDomain: PlatformCustomDomainResponse,
): string {
  return [
    `To verify the custom domain ${vanityDomain.domain}, set the following records on your DNS provider:`,
    ...dnsRecords(vanityDomain).map(
      (r) => `Type: ${r.type}\nName: ${r.name}\nValue: ${r.value}`,
    ),
    VERIFICATION_DELAY_NOTE,
  ].join("\n\n");
}

function withHost(label: string, url: string | null): string {
  return url === null ? label : `${label} (${new URL(url).host})`;
}

export function CustomDomains({
  team,
  deployment,
  entitlements,
}: {
  team: TeamResponse;
  deployment: PlatformDeploymentResponse;
  entitlements: TeamEntitlementsResponse;
}) {
  const hasEntitlement = entitlements.customDomainsEnabled ?? false;

  const vanityDomains = useListVanityDomains(
    hasEntitlement ? deployment.name : undefined,
  );

  const canViewData = useIsOperationAllowed("ViewData");

  return (
    <div className="flex flex-col gap-4">
      <CustomDomainsForm
        team={team}
        deployment={deployment}
        vanityDomains={vanityDomains}
        hasEntitlement={hasEntitlement}
      />

      {canViewData === false ? (
        <Sheet>
          <NoPermissionMessage
            message="You do not have permission to view canonical URL overrides."
            missingPermission="deployment:data:view"
          />
        </Sheet>
      ) : (
        <CanonicalDomainForm vanityDomains={vanityDomains} />
      )}
    </div>
  );
}

export function CustomDomainsForm({
  team,
  deployment,
  vanityDomains,
  hasEntitlement,
}: {
  team: TeamResponse;
  deployment: PlatformDeploymentResponse;
  vanityDomains: PlatformCustomDomainResponse[] | undefined;
  hasEntitlement: boolean;
}) {
  const hasAdminPermissions = useHasProjectAdminPermissions(
    deployment.projectId,
  );
  const project = useCurrentProject();
  const resource =
    project && deployment.kind === "cloud"
      ? deploymentResource(project, {
          id: deployment.id,
          deploymentType: deployment.deploymentType,
          creator: deployment.creator ?? null,
        })
      : undefined;
  // Built-in admin/developer members keep the historical prod-only
  // gate (developers can manage non-prod; admins anywhere). Custom-role
  // members start with no permissions, so the
  // `deployment:customDomain:create`/`delete` grants are required on
  // every deployment type — encoding that into `nonCustomRoleResult`
  // collapses both rules into the admin-or-custom check below.
  const isProd = deployment.deploymentType === "prod";
  const canCreateCustom = useHasCustomRolePermission(
    team.id,
    "deployment:customDomain:create",
    resource,
    !isProd,
  );
  const canDeleteCustom = useHasCustomRolePermission(
    team.id,
    "deployment:customDomain:delete",
    resource,
    !isProd,
  );

  const canCreate = hasAdminPermissions || canCreateCustom === true;
  const canDelete = hasAdminPermissions || canDeleteCustom === true;
  const hasEditAccess = hasEntitlement && canCreate;
  const defaultProdDeployment = useDeployments(
    deployment.projectId,
  ).deployments?.find(
    (d) => d.kind === "cloud" && d.deploymentType === "prod" && d.isDefault,
  );
  const isNonProd = deployment.deploymentType !== "prod";

  return (
    <Sheet>
      <div className="flex flex-col gap-4">
        <div>
          <h3 className="mb-2">Custom Domains</h3>
          <p className="max-w-prose text-pretty">
            Configure custom domains to this deployment. Domains for the Convex
            API (your queries, mutations, and actions) and your HTTP actions are
            configured separately.
          </p>
        </div>

        {isNonProd && (
          <Callout variant="hint">
            <div>
              You can add custom domains to this deployment, but you may be
              looking to configure them on your production deployment instead.{" "}
              {defaultProdDeployment && project ? (
                <Link
                  href={`/t/${team.slug}/${project.slug}/${defaultProdDeployment.name}/settings/custom-domains`}
                >
                  Go to production custom domains.
                </Link>
              ) : null}
            </div>
          </Callout>
        )}

        <div>
          {!hasEntitlement && (
            <>
              <Callout>
                <div>
                  Custom domains are{" "}
                  <span className="font-semibold">
                    only available on the Pro plan
                  </span>
                  .{" "}
                  <Link href={`/t/${team.slug}/settings/billing`}>
                    Upgrade to get access.
                  </Link>
                </div>
              </Callout>
              <LocalDevCallout
                className="flex-col"
                tipText="Tip: Run this to enable custom domains locally:"
                command={`just big-brain-tool-dev entitlement grant add --team-entitlement custom_domains_enabled --team-id ${team.id} --reason "local" true --for-real && just big-brain-tool-dev entitlement grant add --team-entitlement max_custom_domains --team-id ${team.id} --reason "local" 100 --for-real`}
              />
            </>
          )}

          <VanityDomainForm
            disabled={!hasEditAccess}
            disabledTip={
              !hasEntitlement
                ? undefined
                : permissionDeniedTip(
                    "You do not have permission to add custom domains.",
                    "deployment:customDomain:create",
                  )
            }
            deploymentName={deployment.name}
            deploymentUrl={
              deployment.kind === "cloud" ? deployment.deploymentUrl : null
            }
          />
          {vanityDomains && vanityDomains.length > 0 && (
            <>
              <div
                className={classNames(
                  "hidden md:grid",
                  ENVIRONMENT_VARIABLES_ROW_CLASSES,
                )}
              >
                <div
                  className={`flex flex-col gap-1 ${ENVIRONMENT_VARIABLE_NAME_COLUMN}`}
                >
                  <span className="text-xs text-content-secondary">Domain</span>
                </div>
                <div className="flex flex-col gap-1">
                  <span className="text-xs text-content-secondary">
                    Request Destination{" "}
                  </span>
                </div>
              </div>
              <div className="divide-y divide-border-transparent border-t">
                {vanityDomains
                  .sort((a, b) => a.creationTime - b.creationTime)
                  .reverse()
                  .map((domain, index) => (
                    <DisplayVanityDomain
                      key={index}
                      vanityDomain={domain}
                      enabled={hasEntitlement && canDelete}
                      deleteDisabledTip={permissionDeniedTip(
                        "You do not have permission to delete custom domains.",
                        "deployment:customDomain:delete",
                      )}
                    />
                  ))}
              </div>
            </>
          )}
        </div>
      </div>
    </Sheet>
  );
}

export function CanonicalDomainForm({
  vanityDomains,
}: {
  vanityDomains?: PlatformCustomDomainResponse[];
}) {
  const deploymentUrl = useDeploymentUrl();
  const siteUrl = defaultSiteUrl(deploymentUrl);
  const canonicalCloudUrl = useQuery(udfs.convexCloudUrl.default);
  const canonicalSiteUrl = useQuery(udfs.convexSiteUrl.default);

  return (
    <Sheet>
      <div className="flex flex-col gap-3">
        <h3>Override Environment Variables</h3>
        <p className="max-w-prose">
          Replace your system environment variables everywhere in your app. All
          internal references to your <code>CONVEX_SITE_URL</code> and{" "}
          <code>CONVEX_CLOUD_URL</code> will change to the url you set. This can
          affect WebSocket and HTTP clients, storage urls, and Convex Auth.
        </p>
        <CanonicalUrlCombobox
          label={
            <span className="flex items-center gap-2">
              <code>process.env.CONVEX_CLOUD_URL</code>
            </span>
          }
          defaultUrl={{ kind: "default", url: deploymentUrl }}
          canonicalUrl={
            canonicalCloudUrl === undefined
              ? { kind: "loading" }
              : { kind: "loaded", url: canonicalCloudUrl }
          }
          vanityDomains={vanityDomains}
          requestDestination="convexCloud"
        />
        <CanonicalUrlCombobox
          label={
            <span className="flex flex-row items-center gap-1">
              <code>process.env.CONVEX_SITE_URL</code>
            </span>
          }
          defaultUrl={
            siteUrl === null
              ? { kind: "unknownDefault" }
              : { kind: "default", url: siteUrl }
          }
          canonicalUrl={
            canonicalSiteUrl === undefined
              ? { kind: "loading" }
              : { kind: "loaded", url: canonicalSiteUrl }
          }
          vanityDomains={vanityDomains}
          requestDestination="convexSite"
        />
      </div>
    </Sheet>
  );
}

type DefaultUrlOption =
  | {
      // The default url for this deployment, before any overrides were applied.
      kind: "default";
      url: string;
    }
  | {
      // In this case, we don't know what the default url should be.
      // In some cases the deployment doesn't expose its default url through an API.
      kind: "unknownDefault";
    };
type UrlOption =
  | {
      // Either a default url or a canonical url that has yet to be loaded
      // from the deployment. This should be a transient state.
      kind: "loading";
    }
  | DefaultUrlOption
  | {
      // The current canonical url for the deployment, which doesn't match the
      // default or any custom domains.
      // If you switch away from this URL, you probably won't be able to switch
      // back to it.
      kind: "disconnectedCanonical";
      url: string;
    }
  | {
      kind: "custom";
      url: string;
    };

type CanonicalUrl =
  | {
      kind: "loading";
    }
  | {
      kind: "loaded";
      url: string;
    };

export function CanonicalUrlCombobox({
  label,
  defaultUrl,
  canonicalUrl,
  vanityDomains,
  requestDestination,
}: {
  label: ReactNode;
  defaultUrl: DefaultUrlOption;
  canonicalUrl: CanonicalUrl;
  vanityDomains?: PlatformCustomDomainResponse[];
  requestDestination: "convexCloud" | "convexSite";
}) {
  const vanityDomainsForRequestDestination = useMemo(
    () =>
      vanityDomains?.filter(
        (v) => v.requestDestination === requestDestination,
      ) || [],
    [vanityDomains, requestDestination],
  );
  const canonicalIsKnownDefault =
    canonicalUrl.kind === "loaded" &&
    defaultUrl.kind === "default" &&
    canonicalUrl.url === defaultUrl.url;
  const canonicalIsCustom =
    canonicalUrl.kind === "loaded" &&
    vanityDomainsForRequestDestination.some(
      (v) => `https://${v.domain}` === canonicalUrl.url,
    );
  // If we don't know what the default should be, and we don't recognize the canonical URL,
  // assume that the canonical URL is the default.
  const canonicalIsUnknownDefault =
    canonicalUrl.kind === "loaded" &&
    defaultUrl.kind === "unknownDefault" &&
    !canonicalIsCustom;
  const canonicalIsDefault =
    canonicalIsKnownDefault || canonicalIsUnknownDefault;
  const effectiveDefaultUrl: UrlOption = useMemo(() => {
    if (canonicalIsUnknownDefault) {
      return { kind: "default", url: canonicalUrl.url };
    }
    return defaultUrl;
  }, [canonicalIsUnknownDefault, canonicalUrl, defaultUrl]);
  const canonicalUrlOption: UrlOption = useMemo(() => {
    if (canonicalUrl.kind === "loading") {
      return { kind: "loading" };
    }
    if (canonicalIsCustom) {
      return { kind: "custom", url: canonicalUrl.url };
    }
    if (canonicalIsDefault) {
      return { kind: "default", url: canonicalUrl.url };
    }
    return { kind: "disconnectedCanonical", url: canonicalUrl.url };
  }, [canonicalIsCustom, canonicalIsDefault, canonicalUrl]);
  const updateCanonicalUrl = useUpdateCanonicalUrl(requestDestination);
  const options: Option<UrlOption>[] = useMemo(
    () => [
      {
        label:
          effectiveDefaultUrl.kind === "default"
            ? `${effectiveDefaultUrl.url} (default)`
            : "default",
        value: effectiveDefaultUrl,
      },
      ...(canonicalUrlOption.kind === "disconnectedCanonical"
        ? [
            {
              label: `${canonicalUrlOption.url} (disconnected)`,
              value: canonicalUrlOption,
            },
          ]
        : []),
      ...vanityDomainsForRequestDestination.map((v) => ({
        label: `https://${v.domain}${
          v.verificationTime ? "" : " (unverified)"
        }`,
        value: { kind: "custom" as const, url: `https://${v.domain}` },
      })),
    ],
    [
      effectiveDefaultUrl,
      canonicalUrlOption,
      vanityDomainsForRequestDestination,
    ],
  );
  const disabled = options.length <= 1;

  if (canonicalUrlOption.kind === "loading") {
    return <Loading className="h-8 w-full" fullHeight={false} />;
  }

  return (
    <div className="flex flex-col gap-1">
      <Combobox
        label={label}
        labelHidden={false}
        disabled={disabled}
        buttonProps={{
          tip: disabled ? `Add a custom domain first` : undefined,
        }}
        buttonClasses="w-fit"
        optionsWidth="fit"
        options={options}
        selectedOption={canonicalUrlOption}
        setSelectedOption={async (value: UrlOption | null) => {
          if (value === null || value.kind === "loading") {
            captureMessage(
              "Unexpected value selected in CanonicalUrlCombobox",
              "error",
            );
            return;
          }
          await updateCanonicalUrl(
            value.kind === "default" || value.kind === "unknownDefault"
              ? null
              : value.url,
          );
        }}
        disableSearch
      />
    </div>
  );
}

function VanityDomainForm({
  deploymentName,
  deploymentUrl,
  disabled,
  disabledTip,
}: {
  deploymentName: string;
  deploymentUrl: string | null;
  disabled?: boolean;
  disabledTip?: ReactNode;
}) {
  const createVanityDomain = useCreateVanityDomain(deploymentName);
  const siteUrl = deploymentUrl === null ? null : defaultSiteUrl(deploymentUrl);
  const formState = useFormik<PlatformDeleteCustomDomainArgs>({
    validateOnChange: true,
    initialValues: {
      domain: "",
      requestDestination: "convexSite",
    },
    validate: (values: { domain?: string }) => {
      const errors: Partial<PlatformDeleteCustomDomainArgs> = {};
      if (
        !values.domain ||
        values.domain === "" ||
        !values.domain.includes(".")
      ) {
        errors.domain = "Enter a valid domain name";
      }
      return errors;
    },
    onSubmit: async (values: PlatformDeleteCustomDomainArgs) => {
      await createVanityDomain({
        domain: values.domain,
        requestDestination: values.requestDestination,
      });
      formState.resetForm();
    },
  });

  return (
    <form
      className="mb-4 flex flex-col gap-2 overflow-x-clip pt-2 md:flex-row md:items-start"
      onSubmit={(e) => {
        e.preventDefault();
        formState.handleSubmit();
      }}
    >
      <div className="flex w-full flex-col gap-2 md:flex-row">
        <Tooltip
          tip={
            disabled
              ? (disabledTip ??
                "You do not have permission to add custom domains.")
              : undefined
          }
          className="flex grow flex-col gap-1"
        >
          <TextInput
            placeholder="Custom domain URL"
            error={formState.errors.domain}
            onChange={formState.handleChange}
            value={formState.values.domain}
            id="domain"
            labelHidden
            disabled={disabled}
          />
        </Tooltip>
        <Tooltip
          tip={
            disabled
              ? (disabledTip ??
                "You do not have permission to add custom domains.")
              : undefined
          }
        >
          <Combobox
            buttonClasses="w-fit"
            optionsWidth="full"
            label="Request Destination"
            options={[
              {
                label: withHost("HTTP Actions", siteUrl),
                value: "convexSite",
              },
              {
                label: withHost("Convex API", deploymentUrl),
                value: "convexCloud",
              },
            ]}
            selectedOption={formState.values.requestDestination}
            setSelectedOption={async (
              value: "convexSite" | "convexCloud" | null,
            ) => {
              if (value === null) {
                return;
              }
              await formState.setFieldValue("requestDestination", value);
            }}
            disableSearch
            disabled={disabled}
          />
        </Tooltip>
      </div>
      <Button
        className="flex w-fit"
        type="submit"
        disabled={
          disabled ||
          formState.isSubmitting ||
          !formState.isValid ||
          formState.values.domain === ""
        }
        tip={
          disabled
            ? (disabledTip ??
              "You do not have permission to add custom domains.")
            : undefined
        }
        icon={<PlusIcon />}
        variant="primary"
      >
        Add Domain
      </Button>
    </form>
  );
}

export function DisplayVanityDomain({
  vanityDomain,
  enabled,
  deleteDisabledTip,
}: {
  vanityDomain: PlatformCustomDomainResponse;
  enabled: boolean;
  deleteDisabledTip?: ReactNode;
}) {
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  return (
    <div className="flex flex-col">
      <div className={ENVIRONMENT_VARIABLES_ROW_CLASSES}>
        <div
          className={`flex flex-col gap-1 ${ENVIRONMENT_VARIABLE_NAME_COLUMN}`}
        >
          <div className="flex h-9.5 items-center truncate text-content-primary md:col-span-1">
            {vanityDomain.domain}
            {vanityDomain.verificationTime && (
              <Tooltip
                tip="This domain is verified and can receive traffic."
                side="right"
              >
                <CheckCircledIcon className="m-3 text-green-700 dark:text-green-200" />
              </Tooltip>
            )}
          </div>
        </div>

        <div className="flex h-9.5 items-center gap-1">
          <span className="truncate text-content-primary">
            {vanityDomain.requestDestination === "convexCloud"
              ? "Convex API"
              : "HTTP Actions"}
          </span>
        </div>
        <div className="flex h-full items-center justify-end gap-2">
          <Button
            tip={
              !enabled
                ? (deleteDisabledTip ??
                  "You do not have permission to delete custom domains.")
                : "Delete"
            }
            type="button"
            onClick={() => setShowDeleteModal(true)}
            variant="danger"
            size="sm"
            inline
            icon={<TrashIcon />}
            disabled={!enabled}
          />
          {showDeleteModal && (
            <DeleteDomainModal
              onClose={() => setShowDeleteModal(false)}
              domain={vanityDomain}
            />
          )}
        </div>
      </div>
      {!vanityDomain.verificationTime && (
        <>
          <Callout className="mt-0 mb-4 w-72 gap-2 align-middle">
            <div className="ml-1 flex w-full gap-3">
              <ExclamationTriangleIcon className="mt-1" />
              <span className="font-semibold">
                This domain is not verified yet.
              </span>
            </div>
          </Callout>
          <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
            <span className="font-semibold">
              Set the following records on your DNS provider:
            </span>
            <CopyButton
              text={dnsInstructionsText(vanityDomain)}
              label="Copy instructions"
            />
          </div>
          <div className="rounded-sm border p-2">
            <div className="grid grid-cols-1 p-2 md:grid md:grid-cols-[2fr_6fr_3fr] md:gap-2">
              {["Type", "Name", "Value"].map((header) => (
                <div
                  key={header}
                  className="hidden font-semibold text-content-secondary md:block"
                >
                  {header}
                </div>
              ))}

              {dnsRecords(vanityDomain).map((record) => (
                <Fragment key={record.type}>
                  <code className="truncate font-bold wrap-break-word md:font-normal">
                    {record.type}
                  </code>
                  <code className="truncate wrap-break-word">
                    {record.name}
                  </code>
                  <code className="truncate wrap-break-word">
                    {record.value}
                  </code>
                </Fragment>
              ))}
            </div>
          </div>
          <span className="my-4 font-light">{VERIFICATION_DELAY_NOTE}</span>
        </>
      )}
    </div>
  );
}

function DeleteDomainModal({
  domain,
  onClose,
}: {
  domain: PlatformCustomDomainResponse;
  onClose: () => void;
}) {
  const deleteVanityDomain = useDeleteVanityDomain(domain.deploymentName);
  const handleDelete = async () => {
    await deleteVanityDomain({
      domain: domain.domain,
      requestDestination: domain.requestDestination,
    });
  };

  return (
    <ConfirmationDialog
      onClose={onClose}
      onConfirm={handleDelete}
      confirmText="Delete"
      dialogTitle="Delete Custom Domain"
      variant="danger"
      dialogBody={
        <>
          Are you sure you want to delete the custom domain{" "}
          <span className="font-semibold">{domain.domain}</span>?
        </>
      }
    />
  );
}
