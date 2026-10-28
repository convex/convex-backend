import Image from "next/image";
import { useCurrentTheme } from "../useCurrentTheme";

export function PostHogLogo({
  className,
  size,
}: {
  className?: string;
  size: number;
}) {
  const currentTheme = useCurrentTheme();
  const image =
    currentTheme === "dark" ? "/posthog-icon-light.svg" : "/posthog-icon.svg";
  return (
    <Image
      className={className}
      src={image}
      alt="PostHog logo"
      width={size}
      height={size}
    />
  );
}
