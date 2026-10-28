import { Meta, StoryObj } from "@storybook/nextjs";
import { InvoiceResponse } from "generatedApi";
import { Invoices } from "./Invoices";

const meta = {
  component: Invoices,
} satisfies Meta<typeof Invoices>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NoInvoices: Story = {
  args: {
    invoices: [],
  },
};

const statuses: InvoiceResponse["status"][] = [
  "draft",
  "issued",
  "paid",
  "synced",
];

function generateInvoices(num: number): InvoiceResponse[] {
  return Array.from({ length: num }, (_, i) => {
    const status = statuses[i % statuses.length];
    return {
      id: `${i}`,
      invoiceNumber: `INV-${1000 + i}`,
      amountDue: `${((i + 1) * 137.5).toFixed(2)}`,
      total: `${((i + 1) * 137.5).toFixed(2)}`,
      invoiceDate: new Date(2024, 11 - i, 1).getTime(),
      status,
      currency: "USD",
      // A failed payment only happens on an issued (unpaid) invoice.
      hasFailedPayment: status === "issued" && i % 8 === 1,
      hostedInvoiceUrl: i % 5 === 4 ? null : "https://example.com/invoice",
    };
  });
}

// Shows one of each badge state: Upcoming, Due, Paid, Paid (synced).
export const OneOfEachStatus: Story = {
  args: {
    invoices: generateInvoices(4),
  },
};

export const WithFailedPayment: Story = {
  args: {
    invoices: [
      {
        id: "failed",
        invoiceNumber: "INV-2048",
        amountDue: "412.00",
        total: "412.00",
        invoiceDate: new Date(2024, 10, 1).getTime(),
        status: "issued",
        currency: "USD",
        hasFailedPayment: true,
        hostedInvoiceUrl: "https://example.com/invoice",
      },
      ...generateInvoices(4),
    ],
  },
};

export const WithInvoices: Story = {
  args: {
    invoices: generateInvoices(5),
  },
};

// Shows the "why you might get more than one invoice a month" explainer that
// appears when the subscription has a mid-cycle invoicing threshold.
export const WithInvoicingThreshold: Story = {
  args: {
    invoices: generateInvoices(5),
    invoicingThreshold: "10.00",
  },
};

export const WithPagesOfInvoices: Story = {
  args: {
    invoices: generateInvoices(55),
    onShowMore: () => {},
  },
};

export const LoadingMore: Story = {
  args: {
    invoices: generateInvoices(10),
    onShowMore: () => {},
    isLoadingMore: true,
  },
};
