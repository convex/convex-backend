import * as Yup from "yup";
import React, { useCallback, useEffect, useId } from "react";
import { TextInput } from "@ui/TextInput";
import { Form, Formik, getIn, useFormikContext } from "formik";
import { Button } from "@ui/Button";
import { useSetSpendingLimit } from "api/billing";
import { Loading } from "@ui/Loading";
import { Checkbox } from "@ui/Checkbox";
import { cn } from "@ui/cn";
import {
  ExclamationTriangleIcon,
  QuestionMarkCircledIcon,
} from "@radix-ui/react-icons";
import { Tooltip } from "@ui/Tooltip";
import { Link } from "@ui/Link";
import { formatUsd } from "@common/lib/utils";
import { TeamResponse } from "generatedApi";
import * as Sentry from "@sentry/nextjs";

export type SpendingLimitsValue = {
  // null = disabled (= checkbox unchecked)
  // "" = enabled but no value set

  spendingLimitWarningThresholdUsd: null | "" | number;
  spendingLimitDisableThresholdUsd: null | "" | number;
};

export function spendingLimitsSchema({
  currentSpending,
}: {
  currentSpending:
    | { totalCents: number; nextBillingPeriodStart: string }
    | undefined;
}) {
  const baseSchema = Yup.mixed()
    .test(
      "is-spending-value",
      "Please enter a positive number.",
      (value) =>
        value === null ||
        value === "" ||
        (typeof value === "number" && value >= 0),
    )
    .test(
      "is-integer-or-null",
      "Please enter an integer amount.",
      (value) => value === null || value === "" || Number.isInteger(value),
    );

  const disableSchema = baseSchema.test(
    "is-greater-than-current-spending",
    currentSpending !== undefined
      ? `The disable threshold must be greater than the spending in the current billing cycle (${formatUsd(currentSpending.totalCents / 100)}). ${
          currentSpending.nextBillingPeriodStart
            ? `You will be able to lower it at the start of the next billing cycle (${formatDate(
                currentSpending.nextBillingPeriodStart,
              )} at midnight UTC).`
            : ""
        }`
      : "",
    (value) =>
      currentSpending === undefined ||
      value === null ||
      value === "" ||
      currentSpending.totalCents <= value * 100,
  );

  return Yup.object().shape({
    spendingLimitDisableThresholdUsd: disableSchema,
    spendingLimitWarningThresholdUsd: baseSchema.test(
      "is-less-than-spend-limit",
      "The warning threshold must be less than the disable threshold.",
      function isLessThanSpendLimitValidator(warningThreshold) {
        const { parent } = this;
        const disableThreshold = parent.spendingLimitDisableThresholdUsd;

        // If the disable threshold is 0, this field should always be valid
        // (it will be hidden in the UI and set to null)
        if (disableThreshold === 0) {
          return true;
        }

        return (
          typeof warningThreshold !== "number" ||
          typeof disableThreshold !== "number" ||
          warningThreshold < disableThreshold
        );
      },
    ),
  });
}

export function useSubmitSpendingLimits(team: TeamResponse) {
  const setSpendingLimit = useSetSpendingLimit(team.id);

  return useCallback(
    async (v: SpendingLimitsValue) => {
      await setSpendingLimit(spendingLimitValueToCents(v));
    },
    [setSpendingLimit],
  );
}

export function spendingLimitValueToCents(value: SpendingLimitsValue) {
  let { spendingLimitWarningThresholdUsd, spendingLimitDisableThresholdUsd } =
    value;

  if (spendingLimitWarningThresholdUsd === "") {
    spendingLimitWarningThresholdUsd = null;
    Sentry.captureMessage(
      "Spending limits form submitted with empty warning threshold",
      "error",
    );
  }

  if (spendingLimitDisableThresholdUsd === "") {
    spendingLimitDisableThresholdUsd = null;
    Sentry.captureMessage(
      "Spending limits form submitted with empty disable threshold",
      "error",
    );
  }

  return {
    warningThresholdCents:
      typeof spendingLimitWarningThresholdUsd === "number"
        ? spendingLimitWarningThresholdUsd * 100
        : spendingLimitWarningThresholdUsd,
    disableThresholdCents:
      typeof spendingLimitDisableThresholdUsd === "number"
        ? spendingLimitDisableThresholdUsd * 100
        : spendingLimitDisableThresholdUsd,
  };
}

export function SpendingLimitsForm({
  defaultValue,
  onSubmit,
  onCancel,
  currentSpending,
}: {
  defaultValue: SpendingLimitsValue | undefined;
  onSubmit: (values: SpendingLimitsValue) => Promise<void>;
  onCancel: () => void;
  currentSpending:
    | {
        totalCents: number;
        nextBillingPeriodStart: string;
      }
    | undefined;
}) {
  const isLoading = defaultValue === undefined;

  return (
    <Formik
      enableReinitialize
      initialValues={
        isLoading
          ? {
              spendingLimitWarningThresholdUsd: null,
              spendingLimitDisableThresholdUsd: null,
            }
          : defaultValue
      }
      validationSchema={spendingLimitsSchema({ currentSpending })}
      onSubmit={async (values) => {
        await onSubmit(values);
      }}
    >
      {({ isSubmitting }) => (
        <Form className="flex flex-col items-start gap-4">
          {isLoading ? (
            <Loading className="h-[176px] w-full max-w-64" fullHeight={false} />
          ) : (
            <SpendingLimits />
          )}

          <div className="flex gap-2">
            <Button type="submit" disabled={isLoading || isSubmitting}>
              {isSubmitting
                ? "Saving Spending Limits…"
                : "Save Spending Limits"}
            </Button>
            <Button variant="neutral" onClick={onCancel}>
              Cancel
            </Button>
          </div>
        </Form>
      )}
    </Formik>
  );
}

/** To use within a Formik form with a state that is a superset of `SpendingLimitsValue` */
export function SpendingLimits() {
  const { values, setFieldValue, touched } =
    useFormikContext<SpendingLimitsValue>();
  const { spendingLimitWarningThresholdUsd, spendingLimitDisableThresholdUsd } =
    values;

  useEffect(() => {
    if (spendingLimitDisableThresholdUsd === 0) {
      void setFieldValue("spendingLimitWarningThresholdUsd", null);
    }
  }, [
    spendingLimitDisableThresholdUsd,
    setFieldValue,
    touched.spendingLimitDisableThresholdUsd,
  ]);

  return (
    <div className="flex w-full flex-col gap-4">
      {spendingLimitDisableThresholdUsd !== 0 && (
        <SpendLimitInput
          formKey="spendingLimitWarningThresholdUsd"
          label="Warn when spending exceeds"
          accessibleInputLabel="Warning Threshold"
          disabled={spendingLimitDisableThresholdUsd === 0}
          description={
            spendingLimitWarningThresholdUsd !== null && (
              <>
                If your <UsageDefinition /> exceeds this amount,
                <br />
                admins in your team will be notified by email.
              </>
            )
          }
        />
      )}
      <SpendLimitInput
        formKey="spendingLimitDisableThresholdUsd"
        label="Disable projects when spending exceeds"
        accessibleInputLabel="Disable Threshold"
        description={
          spendingLimitDisableThresholdUsd !== null && (
            <span className="mt-0.5 flex gap-1.5 text-content-warning">
              <ExclamationTriangleIcon />
              <span className="block flex-1">
                If your <UsageDefinition /> exceeds{" "}
                {spendingLimitDisableThresholdUsd === 0
                  ? "the built-in limits of your plan, "
                  : "this amount, "}
                <strong className="font-semibold">
                  all of your team’s projects will be disabled
                </strong>{" "}
                until you raise this threshold. This is not a hard cap: usage
                recorded before your projects are disabled is still billed.
              </span>
            </span>
          )
        }
      />
    </div>
  );
}

function UsageDefinition() {
  return (
    <Tooltip
      tip={
        <>
          Resources used beyond the{" "}
          <Link href="https://www.convex.dev/pricing">
            built-in resources of your plan
          </Link>
          . Seat fees are not counted in your spending limits.
        </>
      }
      side="right"
    >
      <span className="inline-flex items-center gap-0.5">
        <span className="underline decoration-dotted">usage</span>
        <QuestionMarkCircledIcon />
      </span>
    </Tooltip>
  );
}

function SpendLimitInput({
  formKey,
  label,
  accessibleInputLabel,
  description,
  disabled = false,
}: {
  formKey:
    | "spendingLimitWarningThresholdUsd"
    | "spendingLimitDisableThresholdUsd";
  label: string;
  accessibleInputLabel: string;
  description?: React.ReactNode;
  disabled?: boolean;
}) {
  const formState = useFormikContext<SpendingLimitsValue>();
  const error = formState.errors[formKey];
  const value = formState.values[formKey];

  const checkboxId = useId();
  const inputId = useId();

  const inputDisabled = value === null || disabled;

  return (
    <div className="flex max-w-64 flex-col gap-1">
      <label
        className="flex items-center gap-2 text-sm text-content-primary"
        htmlFor={checkboxId}
      >
        <Checkbox
          id={checkboxId}
          checked={value !== null}
          onChange={() => {
            if (value === null) {
              let defaultValue = 10;
              if (formKey === "spendingLimitDisableThresholdUsd") {
                const warningThreshold =
                  formState.values.spendingLimitWarningThresholdUsd;
                if (
                  typeof warningThreshold === "number" &&
                  warningThreshold >= 10
                ) {
                  defaultValue = warningThreshold * 2;
                }
              }
              void formState.setFieldValue(formKey, defaultValue, false);
            } else {
              void formState.setFieldValue(formKey, null, false);
              void formState.setFieldTouched(formKey, false, false);
            }
          }}
          disabled={disabled}
        />
        {label}
      </label>

      <TextInput
        id={inputId}
        type="number"
        {...formState.getFieldProps(formKey)}
        value={value ?? ""}
        label={accessibleInputLabel}
        labelHidden
        description={description}
        min={0}
        step={1}
        error={getIn(formState.touched, formKey) && error}
        leftAddon={
          <div
            className={cn(
              "w-4 text-center text-sm",
              inputDisabled ? "text-content-tertiary" : "text-content-primary",
            )}
          >
            $
          </div>
        }
        rightAddon={
          <div
            className={cn(
              "text-sm",
              inputDisabled ? "text-content-primary" : "text-content-secondary",
            )}
          >
            / month
          </div>
        }
        className="pr-16"
        disabled={inputDisabled}
      />
    </div>
  );
}

function formatDate(dateString: string) {
  const parts = dateString.split("-");
  if (parts.length !== 3) {
    throw new Error("Invalid date string");
  }

  const year = parseInt(parts[0], 10);
  const month = parseInt(parts[1], 10) - 1;
  const day = parseInt(parts[2], 10);

  return new Date(year, month, day).toLocaleDateString(undefined, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}
