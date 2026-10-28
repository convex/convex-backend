import Image from "next/image";
import { useCurrentTheme } from "../useCurrentTheme";

export function SentryLogo({
  className,
  size,
}: {
  className?: string;
  size: number;
}) {
  const currentTheme = useCurrentTheme();
  const image =
    currentTheme === "dark" ? "/sentry-light.png" : "/sentry-dark.png";
  return (
    <Image
      className={className}
      src={image}
      alt="Sentry logo"
      width={size}
      height={size}
    />
  );
}
