import { useState } from "react";
import { cn } from "@ui/cn";
import { providerIconUrl, type Provider } from "./providers";

export function ProviderIcon({ provider }: { provider: Provider }) {
  // Keyed by URL so one missing variant doesn't hide the other, and so a
  // provider change retries icons the previous provider failed to load.
  const [failed, setFailed] = useState<ReadonlySet<string>>(new Set());

  const { iconSlug } = provider;
  if (iconSlug === undefined) {
    return null;
  }

  const variant = (mode: "light" | "dark", className: string) => {
    const src = providerIconUrl(iconSlug, mode);
    return failed.has(src) ? null : (
      // eslint-disable-next-line @next/next/no-img-element
      <img
        src={src}
        alt=""
        width={20}
        height={20}
        className={cn("size-5 shrink-0", className)}
        onError={() => setFailed((slugs) => new Set(slugs).add(src))}
      />
    );
  };

  return (
    <>
      {variant("light", "dark:hidden")}
      {variant("dark", "hidden dark:block")}
    </>
  );
}
