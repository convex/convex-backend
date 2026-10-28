declare module "*.svg" {
  const content: React.FunctionComponent<React.SVGProps<SVGSVGElement>>;
  export default content;
}

import ExpoLogo from "@site/static/img/expo-logo.svg";
import NextJSLogo from "@site/static/img/nextjs-logo.svg";
import HtmlLogo from "@site/static/img/html-logo.svg";
import JsLogo from "@site/static/img/js-logo.svg";
import NodeLogo from "@site/static/img/node-logo.svg";
import BunLogo from "@site/static/img/bun-logo.svg";
import PythonLogo from "@site/static/img/python-logo.svg";
import ReactLogo from "@site/static/img/react-logo.svg";
import RemixLogo from "@site/static/img/remix-logo.svg";
import RustLogo from "@site/static/img/rust-logo.svg";
import SvelteLogo from "@site/static/img/svelte-logo.svg";
import VueLogo from "@site/static/img/vue-logo.svg";
import NuxtLogo from "@site/static/img/nuxt-logo.svg";
import AndroidLogo from "@site/static/img/android-logo.svg";
import SwiftLogo from "@site/static/img/swift-logo.svg";
import TanStackLogo from "@site/static/img/tanstack-logo.svg";
import ClaudeCodeLogo from "@site/static/img/claude-code-logo.svg";
import CursorLogo from "@site/static/img/cursor-logo.svg";
import GitHubCopilotLogo from "@site/static/img/github-copilot-logo.svg";
import CodexLogo from "@site/static/img/codex-logo.svg";
import conductorLogoUrl from "@site/static/img/conductor-logo.png";
import React from "react";
import Link from "@docusaurus/Link";
import { ReactNode } from "@mdx-js/react/lib";
import Heading from "@theme/Heading";

function ConductorLogo({ height = 40 }: { height?: number }) {
  return (
    <img src={conductorLogoUrl} height={height} alt="" aria-hidden="true" />
  );
}

// An inline "Claude Code" reference (icon + name) for use in a sentence,
// e.g. "<ClaudeCodeInlineMention /> gets these rules from the plugin instead."
export function ClaudeCodeInlineMention() {
  return (
    <span className="inline-flex items-baseline">
      <ClaudeCodeLogo
        aria-hidden="true"
        className="size-[1em] shrink-0 self-center"
      />
      &nbsp;<span className="font-semibold">Claude Code</span>
    </span>
  );
}

// An inline "Cursor" reference (icon + name) for use in a sentence,
// e.g. "<CursorInlineMention /> gets these rules from the plugin instead."
export function CursorInlineMention() {
  return (
    <span className="inline-flex items-baseline">
      <CursorLogo
        aria-hidden="true"
        className="size-[1em] shrink-0 self-center"
      />
      &nbsp;<span className="font-semibold">Cursor</span>
    </span>
  );
}

// An inline "Codex" reference (icon + name) for use in a sentence,
// e.g. "<CodexInlineMention /> gets these rules from the plugin instead."
export function CodexInlineMention() {
  return (
    <span className="inline-flex items-baseline">
      <CodexLogo
        aria-hidden="true"
        className="size-[1em] shrink-0 self-center"
      />
      &nbsp;<span className="font-semibold">Codex</span>
    </span>
  );
}

// This is a variant of DocsCardList.tsx specifically for the Quickstarts page.
type Item = {
  docId: string;
  href: string;
  label: string;
  icon?: ReactNode;
  invertIcon?: true;
};

// Add this new type after the existing Item type
type LargeCardItem = {
  href: string;
  title: string;
  description: string;
};

export function DocCardList(props: { items: Item[] }) {
  const { items } = props;
  return (
    <ul className="qs-cards">
      {items.map((item, index) => (
        <li key={index}>
          <CardLink item={item} />
        </li>
      ))}
    </ul>
  );
}

export function CardLink({
  className,
  item,
}: {
  className?: string;
  item: Item;
}) {
  const icon = item.icon;
  return (
    <Link
      href={item.href}
      className={
        "card" +
        (item.invertIcon ? " convex-invert-icon" : "") +
        " " +
        (className ?? "")
      }
    >
      {icon}
      <div>
        <div className="card__title text--truncate" title={item.label}>
          {item.label}
        </div>
      </div>
    </Link>
  );
}

// Add this new component before Quick*List
export function LargeCardList(props: { items: LargeCardItem[] }) {
  return (
    <ul className="large-cards">
      {props.items.map((item, index) => (
        <li key={index}>
          <Link href={item.href} className="large-card">
            <Heading as="h2">{item.title}</Heading>
            <p>{item.description}</p>
          </Link>
        </li>
      ))}
    </ul>
  );
}

// End DocsCardList.tsx variant for Quickstarts page

export function QuickFrameworksList() {
  return (
    <DocCardList
      items={[
        {
          icon: <ReactLogo height={40} />,
          href: "/quickstart/react",
          docId: "quickstart/react",
          label: "React",
        },
        {
          icon: <NextJSLogo height={40} />,
          invertIcon: true,
          href: "/quickstart/nextjs",
          docId: "quickstart/nextjs",
          label: "Next.js",
        },
        {
          icon: <RemixLogo height={40} />,
          invertIcon: true,
          href: "/quickstart/remix",
          docId: "quickstart/remix",
          label: "Remix",
        },
        {
          icon: <TanStackLogo height={40} />,
          invertIcon: true,
          href: "/quickstart/tanstack-start",
          docId: "quickstart/tanstack-start",
          label: "TanStack Start",
        },
        {
          icon: <ExpoLogo height={40} />,
          invertIcon: true,
          href: "/quickstart/react-native",
          docId: "quickstart/react-native",
          label: "React Native",
        },
        {
          icon: <VueLogo height={40} />,
          href: "/quickstart/vue",
          docId: "quickstart/vue",
          label: "Vue",
        },
        {
          icon: <NuxtLogo height={40} />,
          href: "/quickstart/nuxt",
          docId: "quickstart/nuxt",
          label: "Nuxt",
        },
        {
          icon: <SvelteLogo height={40} />,
          href: "/quickstart/svelte",
          docId: "quickstart/svelte",
          label: "Svelte",
        },
        {
          icon: <NodeLogo height={40} />,
          href: "/quickstart/nodejs",
          docId: "quickstart/nodejs",
          label: "Node.js",
        },
        {
          icon: <BunLogo height={40} />,
          href: "/quickstart/bun",
          docId: "quickstart/bun",
          label: "Bun",
        },
        {
          icon: <HtmlLogo height={40} />,
          href: "/quickstart/script-tag",
          docId: "quickstart/script-tag",
          label: "Script tag",
        },
      ]}
    />
  );
}

export function QuickHarnessesList() {
  return (
    <DocCardList
      items={[
        {
          icon: <ClaudeCodeLogo height={40} />,
          href: "/ai/using-claude-code",
          docId: "ai/using-claude-code",
          label: "Claude Code",
        },
        {
          icon: <CodexLogo height={40} />,
          href: "/ai/using-codex",
          docId: "ai/using-codex",
          label: "Codex",
        },
        {
          icon: <CursorLogo height={40} />,
          href: "/ai/using-cursor",
          docId: "ai/using-cursor",
          label: "Cursor",
        },
        {
          icon: <GitHubCopilotLogo height={40} />,
          href: "/ai/using-github-copilot",
          docId: "ai/using-github-copilot",
          label: "GitHub Copilot",
        },
        {
          icon: <ConductorLogo height={40} />,
          href: "/ai/using-conductor",
          docId: "ai/using-conductor",
          label: "Conductor",
        },
      ]}
    />
  );
}

export function AgentPluginsList() {
  return (
    <DocCardList
      items={[
        {
          icon: <ClaudeCodeLogo height={40} />,
          href: "/ai/using-claude-code#install-the-convex-plugin-in-claude-code",
          docId: "ai/using-claude-code",
          label: "Claude Code",
        },
        {
          icon: <CodexLogo height={40} />,
          href: "/ai/using-codex#install-the-convex-plugin",
          docId: "ai/using-codex",
          label: "Codex",
        },
        {
          icon: <CursorLogo height={40} />,
          href: "/ai/using-cursor#install-the-convex-plugin-in-cursor",
          docId: "ai/using-cursor",
          label: "Cursor",
        },
      ]}
    />
  );
}

export function RulesHarnessesList() {
  return (
    <DocCardList
      items={[
        {
          icon: <GitHubCopilotLogo height={40} />,
          href: "/ai/using-github-copilot",
          docId: "ai/using-github-copilot",
          label: "GitHub Copilot",
        },
        {
          icon: <ConductorLogo height={40} />,
          href: "/ai/using-conductor",
          docId: "ai/using-conductor",
          label: "Conductor",
        },
      ]}
    />
  );
}

export function McpHarnessesList() {
  return (
    <DocCardList
      items={[
        {
          icon: <CodexLogo height={40} />,
          href: "/ai/using-codex#setup-the-convex-mcp-server",
          docId: "ai/using-codex",
          label: "Codex",
        },
        {
          icon: <GitHubCopilotLogo height={40} />,
          href: "/ai/using-github-copilot#setup-the-convex-mcp-server",
          docId: "ai/using-github-copilot",
          label: "GitHub Copilot",
        },
        {
          icon: <ConductorLogo height={40} />,
          href: "/ai/using-conductor#setup-the-convex-mcp-server",
          docId: "ai/using-conductor",
          label: "Conductor",
        },
      ]}
    />
  );
}

export function QuickLanguagesList() {
  return (
    <DocCardList
      items={[
        {
          icon: <JsLogo height={40} />,
          href: "/client/javascript/overview",
          docId: "client/javascript",
          label: "JavaScript",
        },
        {
          icon: <PythonLogo height={40} />,
          href: "/quickstart/python",
          docId: "quickstart/python",
          label: "Python",
        },
        {
          icon: <SwiftLogo height={40} />,
          href: "/quickstart/swift",
          docId: "quickstart/swift",
          label: "iOS Swift",
        },
        {
          icon: <AndroidLogo height={40} />,
          href: "/quickstart/android",
          docId: "quickstart/android",
          label: "Android Kotlin",
        },
        {
          icon: <RustLogo height={40} width={40} />,
          href: "/quickstart/rust",
          docId: "quickstart/rust",
          label: "Rust",
        },
      ]}
    />
  );
}
