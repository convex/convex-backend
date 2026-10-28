import { useTheme } from "next-themes";

export function useCurrentTheme() {
  const { forcedTheme, resolvedTheme } = useTheme();
  return forcedTheme ?? resolvedTheme;
}
