import Image from "next/image";
import { useCurrentTheme } from "../useCurrentTheme";

type DatadogLogoProps = {
  className?: string;
  size: number;
};

export function DatadogLogo({ className, size }: DatadogLogoProps) {
  const currentTheme = useCurrentTheme();
  const image =
    currentTheme === "dark" ? "/dd_icon_white.png" : "/dd_icon_rgb.png";
  return (
    <Image
      className={className}
      src={image}
      alt="Datadog logo"
      width={size}
      height={size}
    />
  );
}
