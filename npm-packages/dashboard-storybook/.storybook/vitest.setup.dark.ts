import { setProjectAnnotations } from "@storybook/nextjs-vite";
import { beforeAll } from "vitest";
import * as a11yAddonAnnotations from "@storybook/addon-a11y/preview";
import * as previewAnnotations from "./preview";

const annotations = setProjectAnnotations([
  a11yAddonAnnotations,
  previewAnnotations,
  // `theme` is the global that themeDecorator reads through addon-themes.
  {
    initialGlobals: {
      ...previewAnnotations.default.initialGlobals,
      theme: "dark",
    },
  },
]);

beforeAll(annotations.beforeAll);
