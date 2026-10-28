import React, { useContext, useEffect, useRef } from "react";
import { copyTextToClipboard, toast } from "@common/lib/utils";

export type PaletteCopyAction = {
  label: string;
  getText: () => string;
};

export type PaletteCopyRegistry = {
  register: (
    value: string,
    action: () => PaletteCopyAction | null,
  ) => () => void;
  actionFor: (value: string | undefined) => PaletteCopyAction | null;
  copySelected: () => boolean;
};

function selectedItemValue(): string | undefined {
  return (
    document
      .querySelector('[cmdk-dialog] [cmdk-item][aria-selected="true"]')
      ?.getAttribute("data-value") ?? undefined
  );
}

export function createPaletteCopyRegistry(): PaletteCopyRegistry {
  const actions = new Map<string, () => PaletteCopyAction | null>();

  const actionFor = (value: string | undefined) =>
    (value === undefined ? undefined : actions.get(value)?.()) ?? null;

  type DeregisterAction = () => boolean;
  return {
    register(value, action): DeregisterAction {
      actions.set(value, action);
      return () => actions.delete(value);
    },
    actionFor,
    copySelected() {
      const action = actionFor(selectedItemValue());
      const text = action?.getText();
      if (!action || !text) {
        return false;
      }
      void (async () => {
        try {
          await copyTextToClipboard(text);
        } catch {
          return;
        }
        toast(
          "success",
          `${action.label.charAt(0).toUpperCase()}${action.label.slice(1)} copied to the clipboard.`,
          "command-palette-copy",
        );
      })();
      return true;
    },
  };
}

export const PaletteCopyContext =
  React.createContext<PaletteCopyRegistry | null>(null);

export function useCopyAction(value: string, action: PaletteCopyAction | null) {
  const registry = useContext(PaletteCopyContext);
  const latest = useRef(action);
  latest.current = action;
  useEffect(() => {
    if (!registry) {
      return undefined;
    }
    return registry.register(value, () => latest.current);
  }, [registry, value]);
}
