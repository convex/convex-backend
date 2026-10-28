import Image from "next/image";
import { useCurrentTheme } from "../useCurrentTheme";

export function WorkosLogo({
  className,
  size,
}: {
  className?: string;
  size: number;
}) {
  const currentTheme = useCurrentTheme();
  const image =
    currentTheme === "dark" ? "/workos-blue.svg" : "/workos-blue.svg";
  return (
    <Image
      className={className}
      src={image}
      height={size}
      width={size}
      alt="Fivetran logo"
    />
  );
}
