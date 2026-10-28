import { ExternalLinkIcon } from "@radix-ui/react-icons";
import { forwardRef, PropsWithChildren, useContext } from "react";
import { UrlObject } from "url";
import { cn } from "./cn";
import { UIContext } from "./UIContext";

export const Link = forwardRef<
  HTMLAnchorElement,
  PropsWithChildren<
    Omit<React.AnchorHTMLAttributes<HTMLAnchorElement>, "href"> & {
      href: string | UrlObject;
      passHref?: boolean;
      externalIcon?: boolean;
      /**
       * Please disable underline only when the item stands out clearly as a link.
       * https://www.w3.org/WAI/WCAG22/Techniques/failures/F73
       */
      noUnderline?: boolean;
    }
  >
>(function Link(
  { className, children, externalIcon, noUnderline = false, ...props },
  ref,
) {
  const UILink = useContext(UIContext);
  return (
    <UILink
      className={cn(
        // eslint-disable-next-line no-restricted-syntax -- Link component
        "text-content-link",
        "decoration-content-link/40 hover:decoration-content-link",
        noUnderline ? "hover:underline" : "underline",
        "underline-offset-2",
        externalIcon && "inline-flex items-center gap-1",
        className,
      )}
      ref={ref}
      {...props}
    >
      {children}
      {externalIcon && <ExternalLinkIcon className="size-3 shrink-0" />}
    </UILink>
  );
});
