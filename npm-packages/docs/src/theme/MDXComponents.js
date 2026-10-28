// This is a manually swizzled "MDXComponents" component.
// "MDXComponents" defines what components are available in all .md/.mdx files.
// We are swizzling it to add Admonition into the scope (while it's not supported).
// See https://docusaurus.io/docs/markdown-features/react#mdx-component-scope

// Import the original mapper
import MDXComponents from "@theme-original/MDXComponents";
import Admonition from "@theme/Admonition";
import { DocCardList, CardLink } from "@site/src/DocCardList";
import { StepByStep, Step } from "@site/src/StepByStep";
import { TourGuide, TourStep } from "@site/src/TourGuide";
import { Snippet } from "@site/src/snippet.tsx";
import { Details } from "@site/src/Details.tsx";
import { StackPosts } from "@site/src/StackPosts.tsx";
import BetaAdmonition from "@site/docs/_betaAdmonition.mdx";
import BetaContactUsAdmonition from "@site/docs/_betaContactUsAdmonition.mdx";
import ProFeatureUpsell from "@site/docs/_proFeatureUpsell.mdx";
import { CodeWithCopyButton } from "../CodeWithCopyButton";
import { ErrorExample } from "../ErrorExample";
import { Screenshot } from "../components/Screenshot";

export default {
  // Re-use the default set of tags/components
  ...MDXComponents,
  Details,
  // Add Admonition
  Admonition,
  BetaAdmonition,
  BetaContactUsAdmonition,
  CodeWithCopyButton,
  DocCardList,
  ErrorExample,
  CardLink,
  ProFeatureUpsell,
  StackPosts,
  StepByStep,
  Step,
  Snippet,
  Screenshot,
  TourGuide,
  TourStep,
};
