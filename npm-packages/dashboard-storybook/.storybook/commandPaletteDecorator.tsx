import React, { useEffect } from "react";
import { DecoratorFunction } from "storybook/internal/types";
import { ReactRenderer } from "@storybook/nextjs";
import {
  useCommandPaletteAnchor,
  useCommandPaletteOpen,
} from "../../dashboard/src/elements/CommandPalette";

/**
 * Closes the command palette when a story unmounts.
 *
 * The palette's open state and anchor are module-level globals so any trigger
 * can open it from anywhere. The test runner renders every story into the same
 * document, so a story that leaves the palette open hands the next one an open
 * modal — Radix puts `pointer-events: none` on the body while a dialog is open,
 * and the next story's clicks fail.
 */
function ResetCommandPalette({ children }: React.PropsWithChildren<object>) {
  const [, setOpen] = useCommandPaletteOpen();
  const [, setAnchor] = useCommandPaletteAnchor();
  useEffect(
    () => () => {
      setOpen(false);
      setAnchor(null);
    },
    [setOpen, setAnchor],
  );
  return <>{children}</>;
}

export const commandPaletteDecorator: DecoratorFunction<ReactRenderer> = (
  storyFn,
) => <ResetCommandPalette>{storyFn()}</ResetCommandPalette>;
