import React from "react";
import { MagnifyingGlassIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { KEYCAP_CLASSES } from "@ui/KeyboardShortcut";
import { cn } from "@ui/cn";
import { useCommandPaletteOpen } from "./CommandPalette";
import { usePaletteAnalytics } from "./analytics";

// The header search bar that opens the command palette.
export function CommandPaletteTrigger() {
  const [, setOpen] = useCommandPaletteOpen();
  const { trackOpened } = usePaletteAnalytics();

  return (
    <Button
      variant="unstyled"
      onClick={() => {
        trackOpened("button");
        setOpen(true);
      }}
      className="mx-2 hidden w-56 items-center gap-2 rounded-full border bg-background-secondary px-3 py-1.5 text-sm text-content-tertiary transition-colors hover:bg-background-tertiary md:flex"
    >
      <MagnifyingGlassIcon className="size-4 shrink-0" />
      <span className="select-none">Find anything</span>
      <kbd className={cn(KEYCAP_CLASSES, "ml-auto font-sans text-xs")}>/</kbd>
    </Button>
  );
}
