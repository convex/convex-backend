import { cn } from "@ui/cn";
import type { CSSProperties, ReactNode } from "react";

/**
 * A bordered list with a header row styled like the usage page's plan summary
 * table. Children are the body: rows, or a scrolling container of rows. Rows
 * should use `px-4` so they line up with the header.
 */
export function ListTable({
  header,
  children,
  className,
  style,
  surface = "sheet",
}: {
  header: ReactNode;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  /**
   * What the list sits on. On a sheet or modal the header is tinted; directly
   * on the page it shares the rows' background, like the invoices table.
   */
  surface?: "sheet" | "page";
}) {
  return (
    <div
      className={cn(
        "flex w-full flex-col overflow-hidden rounded-lg border bg-background-secondary",
        className,
      )}
      style={style}
    >
      <div
        className={cn(
          "flex items-center gap-3 border-b px-4 py-2 text-sm text-content-secondary",
          surface === "sheet" && "bg-background-tertiary",
        )}
      >
        {header}
      </div>
      {children}
    </div>
  );
}

/** Secondary text at the right end of a `ListTable` header, e.g. a count. */
export function ListTableHeaderDetail({ children }: { children: ReactNode }) {
  return <span className="ml-auto">{children}</span>;
}
