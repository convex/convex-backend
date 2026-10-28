import { SVGProps } from "react";
import { MagnifyingGlassIcon, ArrowTopRightIcon } from "@radix-ui/react-icons";
import { FingerPrintIcon } from "@heroicons/react/24/outline";
import { cn } from "@ui/cn";

export type IndexKind = "database" | "search" | "vector";

export const INDEX_KIND_LABEL: Record<IndexKind, string> = {
  database: "Database index",
  search: "Search index",
  vector: "Vector index",
};

// Icon for each index kind. Inherits its color (`currentColor`) so callers can
// set it via `className` or the surrounding text color.
export function IndexIcon({
  kind,
  className,
}: {
  kind: IndexKind;
  className?: string;
}) {
  const cls = cn("size-4 shrink-0", className);
  switch (kind) {
    case "search":
      return <MagnifyingGlassIcon className={cls} />;
    case "vector":
      return <ArrowTopRightIcon className={cls} />;
    default:
      return <FingerPrintIcon className={cls} />;
  }
}

// Outline funnel for "filter" affordances.
export function FunnelIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      className={cn("size-4 shrink-0", className)}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M2 3h12L9.5 8.5v5h-3v-5L2 3Z" />
    </svg>
  );
}

// A "field" glyph. Fields can hold any type (not just strings), so a `{}` reads
// better than a text icon — and neither icon set ships a curly-brace icon, so
// render it as monospace text. `whitespace-nowrap` keeps the braces on one line;
// size it via `className`'s font-size.
export function FieldIcon({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-flex items-center justify-center font-mono leading-none whitespace-nowrap",
        className,
      )}
    >
      {"{}"}
    </span>
  );
}

export function PackageVersionIcon({
  className,
  ...props
}: SVGProps<SVGSVGElement>) {
  return (
    <svg
      {...props}
      viewBox="0 0 16 16"
      className={cn("size-4 shrink-0", className)}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinejoin="round"
      strokeLinecap="round"
    >
      <g transform="translate(1.98 -1.98) rotate(-45 8 8)">
        <path d="M2.1 3.7h7.3l4.3 4.3-4.3 4.3H2.1A1.1 1.1 0 0 1 1 11.2V4.8a1.1 1.1 0 0 1 1.1-1.1Z" />
        <circle cx="3.4" cy="8" r="1.05" fill="currentColor" stroke="none" />
      </g>
    </svg>
  );
}

export function FunctionIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      {...props}
      viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg"
      stroke="none"
      strokeWidth="1.5"
      fill="currentColor"
    >
      <path d="M4.77207 20C4.22502 20 3.79437 19.8619 3.48012 19.5858C3.16004 19.3096 3 18.9759 3 18.5847C3 18.3028 3.09602 18.0755 3.28807 17.9029C3.4743 17.7303 3.68671 17.644 3.92532 17.644C4.1581 17.644 4.3676 17.7217 4.55383 17.877C4.73424 18.0381 4.82444 18.2539 4.82444 18.5243C4.82444 18.6681 4.77789 18.8091 4.68477 18.9471C4.58584 19.091 4.53637 19.1888 4.53637 19.2406C4.53637 19.3038 4.56838 19.3528 4.6324 19.3873C4.69059 19.4218 4.76334 19.4391 4.85063 19.4391C5.29874 19.4391 5.63919 19.0852 5.87197 18.3776C6.10475 17.6757 6.58778 15.6217 7.32105 12.2157L8.05432 9.1521H6.74491L6.91077 8.41855H8.22017C8.32493 7.9813 8.54607 7.38871 8.88361 6.64078C9.22696 5.88709 9.66343 5.28012 10.193 4.81985C10.8332 4.27328 11.4908 4 12.1659 4C12.7071 4 13.1494 4.12082 13.4927 4.36246C13.8419 4.6041 14.0165 4.96368 14.0165 5.44121C14.0165 5.72312 13.9263 5.95038 13.7459 6.12298C13.5713 6.28982 13.3618 6.37325 13.1174 6.37325C12.8788 6.37325 12.6722 6.28982 12.4976 6.12298C12.323 5.95613 12.2357 5.76052 12.2357 5.53614C12.2357 5.3808 12.2823 5.22258 12.3754 5.06149C12.4743 4.89464 12.5238 4.79684 12.5238 4.76807C12.5238 4.7278 12.5034 4.67602 12.4627 4.61273C12.4161 4.5552 12.3346 4.52643 12.2182 4.52643C11.8923 4.52643 11.5955 4.75081 11.3278 5.19957C11.066 5.64833 10.8215 6.30421 10.5946 7.16721L10.2629 8.41855H11.9214L11.7556 9.1521H10.1406C9.29098 13.283 8.58972 15.9928 8.03686 17.2816C7.25703 19.0939 6.16877 20 4.77207 20Z" />
      <path d="M12.0786 16.3495L13.676 10.7055C13.7401 10.4754 13.7866 10.2941 13.8157 10.1618C13.8739 9.91442 13.903 9.74182 13.903 9.64401C13.903 9.43114 13.8361 9.29881 13.7022 9.24703C13.5684 9.19525 13.3094 9.16361 12.9253 9.1521V8.68608C13.6702 8.64006 14.2493 8.58828 14.6625 8.53074C15.0757 8.47321 15.6925 8.36677 16.5131 8.21143L15.4306 11.4822L15.4743 11.5081C15.4452 11.5369 15.6198 11.2751 15.9981 10.7228C16.3763 10.1647 16.7371 9.71305 17.0805 9.36785C17.418 9.0169 17.7701 8.73786 18.1368 8.53074C18.5092 8.31787 18.8991 8.21143 19.3065 8.21143C19.6848 8.21143 20.0049 8.31787 20.2667 8.53074C20.5344 8.74362 20.6683 9.09169 20.6683 9.57497C20.6683 10.0237 20.4064 11.0133 19.8826 12.5437C19.3531 14.0683 19.0883 14.8853 19.0883 14.9946C19.0883 15.0982 19.1203 15.1787 19.1843 15.2362C19.2483 15.288 19.3123 15.3139 19.3763 15.3139C19.5335 15.3139 19.7662 15.1183 20.0747 14.7271C20.2376 14.52 20.4151 14.2639 20.6072 13.959L21 14.2093C20.5577 14.9169 20.1823 15.4261 19.8739 15.7368C19.3385 16.2718 18.7711 16.5394 18.1717 16.5394C17.8516 16.5394 17.5635 16.4473 17.3075 16.2632C17.0572 16.0791 16.9321 15.7886 16.9321 15.3916C16.9321 15.1384 16.9932 14.7702 17.1154 14.2869C17.2376 13.8037 17.386 13.306 17.5606 12.794L18.451 10.1532C18.4685 10.0899 18.483 10.0352 18.4947 9.98921C18.5063 9.94319 18.5121 9.89716 18.5121 9.85113C18.5121 9.71881 18.4656 9.62675 18.3725 9.57497C18.2852 9.52319 18.1891 9.4973 18.0844 9.4973C17.7177 9.4973 17.3104 9.77922 16.8623 10.343C16.4142 10.9011 16.0475 11.4592 15.7624 12.0173C15.4539 12.6214 15.195 13.2341 14.9855 13.8554C14.7759 14.4768 14.5257 15.3082 14.2347 16.3495H12.0786Z" />
    </svg>
  );
}

export function PuzzlePieceIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      {...props}
      width="13"
      height="10"
      viewBox="0 0 13 10"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path
        d="M9.94362 3.10179C9.94362 3.25832 10.0169 3.40582 10.1417 3.50034C10.2665 3.59486 10.4283 3.6255 10.579 3.58312C10.6981 3.54962 10.8231 3.53334 10.9649 3.53334H10.9702C11.7813 3.53334 12.4395 4.19145 12.4395 5.00263C12.4395 5.8138 11.7813 6.47191 10.9702 6.47191C10.8371 6.47191 10.7063 6.45266 10.579 6.41687C10.4283 6.37449 10.2665 6.40512 10.1417 6.49964C10.0169 6.59416 9.94362 6.74166 9.94362 6.89819V8.75208C9.94362 9.02822 9.71976 9.25208 9.44362 9.25208H1.93945C1.66331 9.25208 1.43945 9.02822 1.43945 8.75208V1.24791C1.43945 0.971767 1.66331 0.74791 1.93945 0.74791H3.87628C3.80708 1.226 3.87402 1.69201 4.09405 2.08387C4.40726 2.64167 4.98769 2.96989 5.7047 2.96989C6.42621 2.96989 6.99353 2.63689 7.29023 2.07136C7.49537 1.68036 7.5506 1.21869 7.484 0.74791H9.44362C9.71976 0.74791 9.94362 0.971767 9.94362 1.24791V3.10179Z"
        stroke="currentColor"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function PulseIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      {...props}
      width="15"
      height="15"
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path
        fill="currentColor"
        d="M9.002 2.5a.75.75 0 01.691.464l6.302 15.305 2.56-6.301a.75.75 0 01.695-.468h4a.75.75 0 010 1.5h-3.495l-3.06 7.532a.75.75 0 01-1.389.004L8.997 5.21l-3.054 7.329A.75.75 0 015.25 13H.75a.75.75 0 010-1.5h4l3.558-8.538a.75.75 0 01.694-.462z"
      />
    </svg>
  );
}

export function WebhookIcon({
  className,
  size,
}: {
  className?: string;
  size: number;
}) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 16 16"
      width={size}
      height={size}
      fill="currentColor"
      className={className}
    >
      <path d="M5.5 4.25a2.25 2.25 0 0 1 4.5 0 .75.75 0 0 0 1.5 0 3.75 3.75 0 1 0-6.14 2.889l-2.272 4.258a.75.75 0 0 0 1.324.706L7 7.25a.75.75 0 0 0-.309-1.015A2.25 2.25 0 0 1 5.5 4.25Z" />
      <path d="M7.364 3.607a.75.75 0 0 1 1.03.257l2.608 4.349a3.75 3.75 0 1 1-.628 6.785.75.75 0 0 1 .752-1.299 2.25 2.25 0 1 0-.033-3.88.75.75 0 0 1-1.03-.256L7.107 4.636a.75.75 0 0 1 .257-1.03Z" />
      <path d="M2.9 8.776A.75.75 0 0 1 2.625 9.8 2.25 2.25 0 1 0 6 11.75a.75.75 0 0 1 .75-.751h5.5a.75.75 0 0 1 0 1.5H7.425a3.751 3.751 0 1 1-5.55-3.998.75.75 0 0 1 1.024.274Z" />
    </svg>
  );
}
