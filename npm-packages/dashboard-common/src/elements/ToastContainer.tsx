import { Toaster } from "sonner";
import { useCurrentTheme } from "../lib/useCurrentTheme";

export function ToastContainer() {
  const resolvedTheme = useCurrentTheme();

  return (
    <Toaster
      theme={resolvedTheme === "dark" ? "dark" : "light"}
      position="bottom-right"
      closeButton
    />
  );
}
