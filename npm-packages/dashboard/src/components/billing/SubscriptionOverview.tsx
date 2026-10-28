import {
  useListInvoices,
  useUpdateBillingAddress,
  useUpdateBillingContact,
  useUpdatePaymentMethod,
  useResumeSubscription,
  useGetCurrentSpend,
  useGetSpendingLimits,
  useListCredits,
} from "api/billing";
import { Loading } from "@ui/Loading";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { BILLING_RESOURCE } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { Button } from "@ui/Button";
import { formatDate } from "@common/lib/format";
import { Sheet } from "@ui/Sheet";
import { useFormik } from "formik";
import { useStripeAddressSetup, useStripePaymentSetup } from "hooks/useStripe";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { Elements } from "@stripe/react-stripe-js";
import { useCallback, useMemo, useRef, useState } from "react";
import { useMount } from "react-use";
import {
  Address,
  BillingContactResponse,
  OrbSubscriptionResponse,
  TeamResponse,
} from "generatedApi";
import { HelpTooltip } from "@ui/HelpTooltip";
import { Callout } from "@ui/Callout";
import { formatUsd } from "@common/lib/utils";
import { planNameMap } from "components/billing/planCards/PlanCard";
import startCase from "lodash/startCase";
import { BillingContactInputs } from "./BillingContactInputs";
import { CreateSubscriptionSchema } from "./UpgradePlanContent";
import { PaymentDetailsForm } from "./PaymentDetailsForm";
import { Invoices } from "./Invoices";
import { PrepaidCredits } from "./PrepaidCredits";
import { SubscriptionCredits } from "./SubscriptionCredits";
import { BillingAddressInputs } from "./BillingAddressInputs";
import {
  SpendingLimitsForm,
  SpendingLimitsValue,
  useSubmitSpendingLimits,
} from "./SpendingLimits";

const DEFAULT_INVOICES_LIMIT = 10;

export function SubscriptionOverview({
  team,
  hasAdminPermissions,
  subscription,
}: {
  team: TeamResponse;
  hasAdminPermissions: boolean;
  subscription?: OrbSubscriptionResponse | null;
}) {
  const isLoading = subscription === undefined;
  const resumeSubscription = useResumeSubscription(team.id);
  const [isResuming, setIsResuming] = useState(false);
  const [invoicesLimit, setInvoicesLimit] = useState<number>(
    DEFAULT_INVOICES_LIMIT,
  );
  const invoicesResult = useListInvoices(team.id, invoicesLimit);
  // `billing:view` gates the billing-detail forms (contact / address /
  // payment method); the surrounding "Current plan" section is readable by
  // all team members.
  const canViewBillingDetails = useHasCustomRolePermission(
    team.id,
    "billing:view",
    BILLING_RESOURCE,
    true,
  );
  // Built-in admin is always allowed to change plans; custom roles need an
  // explicit `billing:subscription:changePlan` grant.
  const canResumeSubscriptionCustom = useHasCustomRolePermission(
    team.id,
    "billing:subscription:changePlan",
    BILLING_RESOURCE,
    false,
  );
  const canResumeSubscription =
    hasAdminPermissions || canResumeSubscriptionCustom;

  if (isLoading) {
    return <Loading className="h-60 w-full" fullHeight={false} />;
  }
  const invoices =
    invoicesResult.status === "ok" ? invoicesResult.data.invoices : undefined;
  const invoicesHasMore =
    invoicesResult.status === "ok" ? invoicesResult.data.hasMore : false;
  const invoicesIsRefreshing =
    invoicesResult.status === "ok" ? invoicesResult.data.isRefreshing : false;
  const nextInvoiceDate = invoices?.find(
    (i) => i.status === "draft",
  )?.invoiceDate;
  const renewalDateContent =
    invoicesResult.status === "loading" ? (
      <Loading
        className="h-4 w-32 bg-neutral-8/30 dark:bg-neutral-3/20"
        fullHeight={false}
      />
    ) : typeof nextInvoiceDate === "number" ? (
      <span className="font-semibold">
        {formatDate(new Date(nextInvoiceDate))}
      </span>
    ) : null;
  const renewalDate = renewalDateContent && (
    <div className="flex items-center gap-1 text-sm">
      Subscription renews on
      {renewalDateContent}
    </div>
  );
  return (
    <>
      {subscription && (
        <Sheet className="flex flex-col gap-4">
          <h3>Subscription</h3>
          <div className="text-sm">
            Current Plan:{" "}
            <span className="font-semibold">
              {subscription.plan.planType
                ? planNameMap[subscription.plan.planType] ||
                  subscription.plan.name
                : subscription.plan.name}
            </span>
          </div>
          {typeof subscription.endDate === "number" ? (
            <>
              <div className="text-sm">
                Subscription ends on{" "}
                <span className="font-semibold">
                  {formatDate(new Date(subscription.endDate))}
                </span>
              </div>
              <Button
                disabled={canResumeSubscription !== true || isResuming}
                className="w-fit"
                tip={
                  canResumeSubscription === false &&
                  permissionDeniedTip(
                    "You do not have permission to modify the team subscription.",
                    "billing:subscription:changePlan",
                  )
                }
                loading={isResuming}
                onClick={async () => {
                  setIsResuming(true);
                  try {
                    await resumeSubscription();
                  } finally {
                    setIsResuming(false);
                  }
                }}
              >
                Resume Subscription
              </Button>
            </>
          ) : (
            renewalDate
          )}
          {/* The backend only populates these financial fields for actors that
              can view billing details. */}
          {canViewBillingDetails === true && (
            <SubscriptionCredits accountBalance={subscription.accountBalance} />
          )}
          <hr />
          <PrepaidCreditsContainer team={team} />
          <SpendingLimitsSectionContainer
            subscription={subscription}
            team={team}
            hasAdminPermissions={hasAdminPermissions}
          />
          {team.managedBy !== "vercel" &&
            canViewBillingDetails === true &&
            subscription.billingContact && (
              <>
                <hr />
                <BillingContactForm
                  billingContact={subscription.billingContact}
                  team={team}
                />
                <hr />
                <BillingAddressForm
                  subscription={subscription}
                  billingContact={subscription.billingContact}
                  team={team}
                />
                <hr />
                <PaymentMethodForm subscription={subscription} team={team} />
              </>
            )}
          {team.managedBy !== "vercel" && canViewBillingDetails === false && (
            <>
              <hr />
              <div className="flex flex-col gap-4">
                <h4>Billing Contact</h4>
                <NoPermissionMessage
                  message="You do not have permission to view the billing contact for this team."
                  missingPermission="billing:view"
                />
              </div>
              <hr />
              <div className="flex flex-col gap-4">
                <h4>Billing Address</h4>
                <NoPermissionMessage
                  message="You do not have permission to view the billing address for this team."
                  missingPermission="billing:view"
                />
              </div>
              <hr />
              <div className="flex flex-col gap-4">
                <h4>Payment Method</h4>
                <NoPermissionMessage
                  message="You do not have permission to view the payment method for this team."
                  missingPermission="billing:view"
                />
              </div>
            </>
          )}
        </Sheet>
      )}
      {team.managedBy !== "vercel" && invoicesResult.status === "loading" && (
        <Loading className="h-60 w-full" fullHeight={false} />
      )}
      {team.managedBy !== "vercel" && invoicesResult.status === "denied" && (
        <Sheet className="flex w-full flex-col gap-4">
          <h3>Invoices</h3>
          <span className="text-sm">
            Preview or download your upcoming and past invoices.
          </span>
          <NoPermissionMessage
            message="You do not have permission to view invoices for this team."
            missingPermission={invoicesResult.deniedAction}
          />
        </Sheet>
      )}
      {team.managedBy !== "vercel" &&
        invoices &&
        (invoices.length > 0 || subscription) && (
          <Invoices
            invoices={invoices}
            invoicingThreshold={subscription?.invoicingThreshold}
            onShowMore={
              invoicesHasMore
                ? () => setInvoicesLimit(invoicesLimit + 10)
                : undefined
            }
            isLoadingMore={invoicesIsRefreshing}
          />
        )}
    </>
  );
}

function PrepaidCreditsContainer({ team }: { team: TeamResponse }) {
  const { promos } = useLaunchDarkly();
  // A null team id pauses the query, so an unflagged team costs no Orb call.
  const creditsResult = useListCredits(promos ? team.id : null);
  if (
    !promos ||
    creditsResult.status !== "ok" ||
    creditsResult.data.length === 0
  ) {
    return null;
  }
  return <PrepaidCredits credits={creditsResult.data} />;
}

function SpendingLimitsSectionContainer({
  subscription,
  team,
  hasAdminPermissions,
}: {
  subscription: OrbSubscriptionResponse;
  team: TeamResponse;
  hasAdminPermissions: boolean;
}) {
  const submitSpendingLimits = useSubmitSpendingLimits(team);
  const canSetSpendingLimitCustom = useHasCustomRolePermission(
    team.id,
    "billing:spendingLimit:update",
    BILLING_RESOURCE,
    false,
  );
  const canSetSpendingLimit = hasAdminPermissions || canSetSpendingLimitCustom;

  const currentSpendResult = useGetCurrentSpend(
    hasAdminPermissions ? team.id : null,
  );
  const totalCents =
    currentSpendResult.status === "ok"
      ? currentSpendResult.data.totalCents
      : undefined;
  const currentSpend = useMemo(() => {
    if (
      totalCents === undefined ||
      subscription.nextBillingPeriodStart === undefined
    ) {
      return undefined;
    }

    return {
      totalCents,
      nextBillingPeriodStart: subscription.nextBillingPeriodStart,
    };
  }, [totalCents, subscription.nextBillingPeriodStart]);

  const spendingLimitsResult = useGetSpendingLimits(team.id);
  if (spendingLimitsResult.status === "denied") {
    return (
      <div className="flex flex-col gap-4">
        <h4>Usage Spending Limits</h4>
        <NoPermissionMessage
          message="You do not have permission to view spending limits for this team."
          missingPermission={spendingLimitsResult.deniedAction}
        />
      </div>
    );
  }
  const spendingLimits =
    spendingLimitsResult.status === "ok"
      ? spendingLimitsResult.data
      : undefined;

  return (
    <SpendingLimitsSection
      currentSpendLimit={spendingLimits}
      currentSpend={currentSpend}
      hasAdminPermissions={canSetSpendingLimit === true}
      onSubmit={submitSpendingLimits}
    />
  );
}

export function SpendingLimitsSection({
  currentSpendLimit,
  currentSpend,
  hasAdminPermissions,
  onSubmit,
}: {
  currentSpendLimit:
    | {
        disableThresholdCents: number | null;
        warningThresholdCents: number | null;
        state: null | "Running" | "Disabled" | "Warning";
      }
    | undefined;
  currentSpend:
    | { totalCents: number; nextBillingPeriodStart: string }
    | undefined;
  hasAdminPermissions: boolean;
  onSubmit: (v: SpendingLimitsValue) => Promise<void>;
}) {
  const [showForm, setShowForm] = useState(false);

  return (
    <div className="flex flex-col gap-4">
      <h4>Usage Spending Limits</h4>

      {currentSpendLimit?.state === "Disabled" && (
        <Callout variant="error">
          Your projects are disabled because you exceeded your spending limit.
          Increase it to re-enable your projects.
        </Callout>
      )}

      {!showForm ? (
        <>
          <div className="flex flex-wrap gap-x-8 gap-y-4">
            {currentSpendLimit === undefined ? (
              <>
                <Loading className="h-12 w-36" fullHeight={false} />
                <Loading className="h-12 w-36" fullHeight={false} />
              </>
            ) : currentSpendLimit.disableThresholdCents === null &&
              currentSpendLimit.warningThresholdCents === null ? (
              <p>You don’t have any spending limits set.</p>
            ) : (
              <>
                {currentSpendLimit.warningThresholdCents !== null && (
                  <CostLabel
                    label="Warning threshold"
                    priceCents={currentSpendLimit.warningThresholdCents}
                    tooltip="If your usage exceeds this amount, admins in your team will be notified by email."
                  />
                )}
                {currentSpendLimit.disableThresholdCents !== null && (
                  <CostLabel
                    label="Disable threshold"
                    priceCents={currentSpendLimit.disableThresholdCents}
                    tooltip={`If your usage exceeds ${currentSpendLimit.disableThresholdCents === 0 ? "the built-in limits of your plan" : "this amount"}, all your projects will be paused.`}
                  />
                )}
              </>
            )}
          </div>

          <Button
            className="w-fit"
            onClick={() => setShowForm(true)}
            variant="neutral"
            disabled={!hasAdminPermissions}
            tip={
              !hasAdminPermissions &&
              permissionDeniedTip(
                "You do not have permission to change your spending limits.",
                "billing:spendingLimit:update",
              )
            }
          >
            {currentSpendLimit === null
              ? "Set spending limits"
              : "Change spending limits"}
          </Button>
        </>
      ) : (
        <SpendingLimitsForm
          defaultValue={
            currentSpendLimit === undefined
              ? undefined
              : {
                  spendingLimitWarningThresholdUsd:
                    currentSpendLimit.warningThresholdCents === null
                      ? null
                      : currentSpendLimit.warningThresholdCents / 100,
                  spendingLimitDisableThresholdUsd:
                    currentSpendLimit.disableThresholdCents === null
                      ? null
                      : currentSpendLimit.disableThresholdCents / 100,
                }
          }
          currentSpending={currentSpend}
          onSubmit={async (v) => {
            await onSubmit(v);
            setShowForm(false);
          }}
          onCancel={() => setShowForm(false)}
        />
      )}
    </div>
  );
}

function CostLabel({
  label,
  priceCents,
  tooltip,
}: {
  label: string;
  priceCents: number;
  tooltip: string;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="flex items-center gap-1 text-content-secondary">
        {label}
        <HelpTooltip tipSide="top">{tooltip}</HelpTooltip>
      </span>
      <span className="flex items-baseline gap-1">
        {/* eslint-disable-next-line no-restricted-syntax */}
        <div className="text-lg font-medium">{formatUsd(priceCents / 100)}</div>
        <span className="text-sm text-content-secondary">/ month</span>
      </span>
    </div>
  );
}

function BillingContactForm({
  billingContact,
  team,
}: {
  billingContact: BillingContactResponse;
  team: TeamResponse;
}) {
  const [showForm, setShowForm] = useState(false);
  const updateBillingContact = useUpdateBillingContact(team.id);
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canUpdateCustom = useHasCustomRolePermission(
    team.id,
    "billing:contact:update",
    BILLING_RESOURCE,
    false,
  );
  const canUpdate = isTeamAdmin || canUpdateCustom;
  const formState = useFormik<BillingContactResponse>({
    initialValues: {
      name: billingContact.name,
      email: billingContact.email,
    },
    validationSchema: CreateSubscriptionSchema,
    onSubmit: async (v) => {
      await updateBillingContact(v);
      await formState.setTouched({});
      setShowForm(false);
    },
    enableReinitialize: true,
  });

  return (
    <div className="flex flex-col gap-4">
      <h4>Billing Contact</h4>
      {!showForm ? (
        <>
          <div className="text-sm">
            <div>
              <span className="font-semibold">{billingContact.name}</span>
            </div>
            <div>{billingContact.email}</div>
          </div>
          <Button
            className="w-fit"
            onClick={() => setShowForm(true)}
            variant="neutral"
            disabled={canUpdate !== true}
            tip={
              canUpdate === false &&
              permissionDeniedTip(
                "You do not have permission to update the billing contact.",
                "billing:contact:update",
              )
            }
          >
            Change billing contact
          </Button>
        </>
      ) : (
        <form
          className="max-w-64"
          onSubmit={(e) => {
            e.preventDefault();
            formState.handleSubmit();
          }}
        >
          <BillingContactInputs
            formState={formState}
            disabled={canUpdate !== true}
          />
          <div className="mt-4 flex gap-2">
            <Button
              type="submit"
              disabled={
                !formState.dirty || !formState.isValid || canUpdate !== true
              }
              tip={
                canUpdate === false &&
                permissionDeniedTip(
                  "You do not have permission to update the billing contact.",
                  "billing:contact:update",
                )
              }
              loading={formState.isSubmitting}
            >
              Save Billing Contact
            </Button>
            <Button
              type="button"
              variant="neutral"
              onClick={() => {
                formState.resetForm();
                setShowForm(false);
              }}
            >
              Cancel
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}

function BillingAddressForm({
  team,
  subscription,
  billingContact,
}: {
  team: TeamResponse;
  subscription: OrbSubscriptionResponse;
  billingContact: BillingContactResponse;
}) {
  const [showForm, setShowForm] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useMount(() => {
    if (window.location.hash === "#billingAddress") {
      ref.current?.scrollIntoView();
    }
  });
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canUpdateCustom = useHasCustomRolePermission(
    team.id,
    "billing:address:update",
    BILLING_RESOURCE,
    false,
  );
  const canUpdate = isTeamAdmin || canUpdateCustom;

  const updateBillingAddress = useUpdateBillingAddress(team.id);
  const formState = useFormik<{ billingAddress?: Address }>({
    initialValues: {
      billingAddress: subscription.billingAddress || undefined,
    },
    onSubmit: async (v) => {
      if (v.billingAddress) {
        await updateBillingAddress({ billingAddress: v.billingAddress });
        await formState.setTouched({});
        setShowForm(false);
      }
    },
    enableReinitialize: true,
  });

  const { setFieldValue } = formState;
  const setBillingAddress = useCallback(
    async (address?: Address) => {
      await setFieldValue("billingAddress", address);
    },
    [setFieldValue],
  );
  const { stripePromise, options } = useStripeAddressSetup(
    team,
    canUpdate === true,
  );

  return (
    <div className="flex flex-col gap-4" ref={ref}>
      <h4>Billing Address</h4>
      {team.managedBy === "vercel" && (
        <Callout>
          <div>
            This team is managed by {startCase(team.managedBy)}. You may add a
            billing address if you wish to upgrade to the Professional plan.
          </div>
        </Callout>
      )}
      {!showForm ? (
        <>
          <div className="text-sm">
            {subscription.billingAddress ? (
              <div>
                <div>
                  {subscription.billingAddress.line1}
                  {subscription.billingAddress.line2 && (
                    <div>{subscription.billingAddress.line2}</div>
                  )}
                  <div>
                    {subscription.billingAddress.city},{" "}
                    {subscription.billingAddress.state}{" "}
                    {subscription.billingAddress.postal_code}
                  </div>
                  <div>{subscription.billingAddress.country}</div>
                </div>
              </div>
            ) : (
              <div>No billing address on file.</div>
            )}
          </div>
          <Button
            className="w-fit"
            onClick={() => setShowForm(true)}
            disabled={canUpdate !== true}
            variant="neutral"
            tip={
              canUpdate === false &&
              permissionDeniedTip(
                "You do not have permission to update the billing address.",
                "billing:address:update",
              )
            }
          >
            {subscription.billingAddress
              ? "Change billing address"
              : "Add billing address"}
          </Button>
        </>
      ) : (
        <form
          className="w-full"
          onSubmit={(e) => {
            e.preventDefault();
            formState.handleSubmit();
          }}
        >
          {canUpdate === true ? (
            options.clientSecret ? (
              <Elements stripe={stripePromise} options={options}>
                <BillingAddressInputs
                  onChangeAddress={setBillingAddress}
                  existingBillingAddress={
                    subscription.billingAddress || undefined
                  }
                  name={billingContact.name}
                />
              </Elements>
            ) : null
          ) : (
            <div className="flex flex-col gap-4">
              <div className="text-sm">
                {subscription.billingAddress ? (
                  <div>
                    <div>
                      {subscription.billingAddress.line1}
                      {subscription.billingAddress.line2 && (
                        <div>{subscription.billingAddress.line2}</div>
                      )}
                      <div>
                        {subscription.billingAddress.city},{" "}
                        {subscription.billingAddress.state}{" "}
                        {subscription.billingAddress.postal_code}
                      </div>
                      <div>{subscription.billingAddress.country}</div>
                    </div>
                  </div>
                ) : (
                  <div>No billing address on file.</div>
                )}
              </div>
            </div>
          )}

          <div className="mt-4 flex gap-2">
            <Button
              type="submit"
              disabled={
                !formState.dirty ||
                !formState.values.billingAddress ||
                canUpdate !== true
              }
              tip={
                canUpdate === false &&
                permissionDeniedTip(
                  "You do not have permission to update the billing address.",
                  "billing:address:update",
                )
              }
              loading={formState.isSubmitting}
            >
              Save Billing Address
            </Button>
            <Button
              type="button"
              variant="neutral"
              onClick={() => {
                formState.resetForm();
                setShowForm(false);
              }}
            >
              Cancel
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}

function PaymentMethodForm({
  team,
  subscription,
}: {
  team: TeamResponse;
  subscription: OrbSubscriptionResponse;
}) {
  const [showForm, setShowForm] = useState(false);
  const onSave = useCallback(() => {
    setShowForm(false);
  }, []);
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canUpdateCustom = useHasCustomRolePermission(
    team.id,
    "billing:paymentMethod:update",
    BILLING_RESOURCE,
    false,
  );
  const canUpdate = isTeamAdmin || canUpdateCustom;

  const ref = useRef<HTMLDivElement>(null);
  useMount(() => {
    if (window.location.hash === "#paymentMethod") {
      ref.current?.scrollIntoView();
    }
  });

  return (
    <div className="flex flex-col gap-4">
      <h4>Payment Method</h4>
      {team.managedBy === "vercel" && (
        <Callout>
          <div>
            This team is managed by {startCase(team.managedBy)}. You may add a
            payment method if you wish to upgrade to the Professional plan.
          </div>
        </Callout>
      )}
      {subscription.paymentMethod && (
        <div className="text-sm">
          Current payment method:{" "}
          <span className="font-semibold">
            {subscription.paymentMethod.display}
          </span>
        </div>
      )}
      {showForm ? (
        <UpdatePaymentMethod team={team} onSave={onSave} />
      ) : (
        <Button
          ref={ref}
          className="w-fit"
          onClick={() => setShowForm(true)}
          disabled={canUpdate !== true}
          variant="neutral"
          tip={
            canUpdate === false &&
            permissionDeniedTip(
              "You do not have permission to update the payment method.",
              "billing:paymentMethod:update",
            )
          }
        >
          {subscription.paymentMethod
            ? "Change payment method"
            : "Add payment method"}
        </Button>
      )}
    </div>
  );
}
function UpdatePaymentMethod({
  team,
  onSave,
}: {
  team: TeamResponse;
  onSave: () => void;
}) {
  const updatePaymentMethod = useUpdatePaymentMethod(team.id);
  const updatePaymentMethodCb = useCallback(
    async (paymentMethod?: string) => {
      if (paymentMethod) {
        await updatePaymentMethod({ paymentMethod });
        onSave();
      }
    },
    [onSave, updatePaymentMethod],
  );
  const { stripePromise, options, confirmSetup } = useStripePaymentSetup(
    team,
    undefined,
    updatePaymentMethodCb,
  );
  const { preAuthorizePaymentMethodChange } = useLaunchDarkly();

  return options.clientSecret ? (
    <Elements stripe={stripePromise} options={options}>
      <PaymentDetailsForm
        confirmSetup={confirmSetup}
        showCardHoldNote={preAuthorizePaymentMethodChange}
      />
    </Elements>
  ) : null;
}
