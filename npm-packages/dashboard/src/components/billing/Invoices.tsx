import {
  ArrowTopRightOnSquareIcon,
  DocumentTextIcon,
} from "@heroicons/react/24/outline";
import { Button } from "@ui/Button";
import { HelpTooltip } from "@ui/HelpTooltip";
import { Sheet } from "@ui/Sheet";
import { cn } from "@ui/cn";
import { formatDate } from "@common/lib/format";
import { formatUsd } from "@common/lib/utils";
import { InvoiceResponse } from "generatedApi";
import startCase from "lodash/startCase";

export function Invoices({
  invoices,
  invoicingThreshold,
  onShowMore,
  isLoadingMore = false,
}: {
  invoices: InvoiceResponse[];
  // The subscription's mid-cycle invoicing threshold in USD (e.g. "10.00"), or
  // null/undefined when the team has no threshold configured.
  invoicingThreshold?: string | null;
  onShowMore?: () => void;
  isLoadingMore?: boolean;
}) {
  return (
    <Sheet className="flex w-full flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h3>Invoices</h3>
        <span className="text-sm text-content-secondary">
          Preview or download your upcoming and past invoices.
        </span>
      </div>
      <InvoicingThresholdExplainer invoicingThreshold={invoicingThreshold} />
      {invoices.length > 0 ? (
        <InvoicesTable
          invoices={invoices}
          onShowMore={onShowMore}
          isLoadingMore={isLoadingMore}
        />
      ) : (
        <EmptyState />
      )}
    </Sheet>
  );
}

function InvoicingThresholdExplainer({
  invoicingThreshold,
}: {
  invoicingThreshold?: string | null;
}) {
  const threshold =
    typeof invoicingThreshold === "string"
      ? parseFloat(invoicingThreshold)
      : NaN;
  if (!Number.isFinite(threshold) || threshold <= 0) {
    // No mid-cycle threshold configured (e.g. the team is invoiced only at the
    // end of each billing period), so there's nothing to explain.
    return null;
  }
  const formatted = formatUsd(threshold);
  return (
    <div className="flex items-center gap-1 text-sm text-content-secondary">
      Invoice threshold:
      <span className="font-medium text-content-primary">{formatted}</span>
      <HelpTooltip tipSide="top">
        {`Usage is invoiced in ${formatted} increments, so you may receive multiple invoices in a single billing period if your usage exceeds ${formatted}. For usage incurred within a short amount of time, invoices may be aggregated to multiple threshold increments.`}
      </HelpTooltip>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-col items-center gap-2 rounded-lg border border-dashed py-16 text-content-secondary">
      <DocumentTextIcon className="size-8 text-content-tertiary" />
      <p className="text-sm">No invoices yet.</p>
      <p className="max-w-xs text-center text-xs text-content-tertiary">
        Invoices will appear here once your first billing period begins.
      </p>
    </div>
  );
}

const cellClass = "px-3 py-2.5 align-middle text-sm";
const headerClass =
  "px-3 py-2 text-left text-xs font-medium text-content-secondary";

function InvoicesTable({
  invoices,
  onShowMore,
  isLoadingMore,
}: {
  invoices: InvoiceResponse[];
  onShowMore?: () => void;
  isLoadingMore: boolean;
}) {
  return (
    <div
      className="scrollbar max-h-120 overflow-y-auto rounded-lg border"
      // Allow scrollable zone to be focused
      // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- https://dequeuniversity.com/rules/axe/4.11/scrollable-region-focusable?application=axeAPI
      tabIndex={0}
    >
      <table className="w-full border-collapse">
        <thead className="sticky top-0 z-10 bg-background-secondary/95 backdrop-blur-sm">
          <tr className="border-b">
            <th className={headerClass}>Invoice</th>
            <th className={headerClass}>Issue date</th>
            <th className={headerClass}>Status</th>
            <th className={cn(headerClass, "text-right")}>Amount</th>
            <th className={cn(headerClass, "w-0")}>
              <span className="sr-only">Receipt</span>
            </th>
          </tr>
        </thead>
        <tbody className="divide-y">
          {invoices.map((invoice) => (
            <InvoiceRow key={invoice.id} invoice={invoice} />
          ))}
        </tbody>
      </table>
      {onShowMore && (
        <div className="flex justify-center border-t bg-background-secondary p-2">
          <Button
            variant="neutral"
            size="xs"
            onClick={onShowMore}
            loading={isLoadingMore}
            disabled={isLoadingMore}
          >
            Show more
          </Button>
        </div>
      )}
    </div>
  );
}

function InvoiceRow({ invoice }: { invoice: InvoiceResponse }) {
  const isUpcoming = invoice.status === "draft";
  const amount = parseFloat(invoice.total).toLocaleString("en-US", {
    currency: invoice.currency,
    style: "currency",
    currencyDisplay: "symbol",
  });

  return (
    <tr className="group transition-colors hover:bg-background-tertiary">
      <td className={cn(cellClass, "font-medium")}>
        {isUpcoming ? (
          <span className="text-content-secondary">Upcoming invoice</span>
        ) : (
          <span className="font-mono text-xs">{invoice.invoiceNumber}</span>
        )}
      </td>
      <td className={cn(cellClass, "whitespace-nowrap text-content-secondary")}>
        {formatDate(new Date(invoice.invoiceDate))}
      </td>
      <td className={cellClass}>
        <StatusBadge invoice={invoice} />
      </td>
      <td className={cn(cellClass, "text-right font-medium tabular-nums")}>
        {amount}
      </td>
      <td className={cn(cellClass, "text-right")}>
        <Button
          tip={
            invoice.hostedInvoiceUrl
              ? isUpcoming
                ? "Preview upcoming invoice"
                : "View invoice"
              : "Could not generate a link to this invoice."
          }
          tipSide="left"
          size="xs"
          variant="neutral"
          inline
          aria-label={isUpcoming ? "Preview upcoming invoice" : "View invoice"}
          icon={<ArrowTopRightOnSquareIcon className="size-4" />}
          disabled={!invoice.hostedInvoiceUrl}
          href={invoice.hostedInvoiceUrl || undefined}
          target="_blank"
          className="opacity-60 transition-opacity group-hover:opacity-100"
        />
      </td>
    </tr>
  );
}

type BadgeStyle = {
  label: string;
  dotClassName: string;
  textClassName: string;
};

function statusBadgeStyle(invoice: InvoiceResponse): BadgeStyle {
  if (invoice.hasFailedPayment) {
    return {
      label: "Payment failed",
      dotClassName: "bg-util-error",
      textClassName: "text-content-error",
    };
  }
  switch (invoice.status) {
    case "paid":
    case "synced":
      return {
        label: "Paid",
        dotClassName: "bg-util-success",
        textClassName: "text-content-primary",
      };
    case "issued":
      return {
        label: "Due",
        dotClassName: "bg-util-warning",
        textClassName: "text-content-primary",
      };
    case "draft":
      return {
        label: "Upcoming",
        dotClassName: "bg-neutral-8 dark:bg-neutral-4",
        textClassName: "text-content-primary",
      };
    default:
      return {
        label: startCase(invoice.status),
        dotClassName: "bg-neutral-8 dark:bg-neutral-4",
        textClassName: "text-content-primary",
      };
  }
}

function StatusBadge({ invoice }: { invoice: InvoiceResponse }) {
  const { label, dotClassName, textClassName } = statusBadgeStyle(invoice);
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 text-xs font-medium whitespace-nowrap",
        textClassName,
      )}
    >
      <span className={cn("size-1.5 shrink-0 rounded-full", dotClassName)} />
      {label}
    </span>
  );
}
