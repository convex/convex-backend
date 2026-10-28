import { themes } from "prism-react-renderer";
const lightCodeTheme = themes.github;
import darkCodeTheme from "./src/theme/prism-theme/oneDark.js";
import * as dotenv from "dotenv";
import { execSync } from "child_process";
import { mkdirSync, writeFileSync } from "fs";
import { join, resolve } from "path";
import type { Config, ThemeConfig } from "@docusaurus/types";
import type { Options as PresetClassicOptions } from "@docusaurus/preset-classic";
import type * as OpenApiPlugin from "docusaurus-plugin-openapi-docs";

// Load environment variables.
dotenv.config({ path: ".env.local" });

const ENTRY_POINTS_TO_DOCUMENT = [
  "browser",
  "server",
  "react",
  "react-auth0",
  "react-clerk",
  "nextjs",
  "values",
];

const config: Config = {
  future: {
    v4: true,
  },
  title: "Convex Developer Hub",
  tagline: "The source for documentation about Convex.",
  url: "https://docs.convex.dev",
  baseUrl: "/",
  onBrokenLinks: "throw",
  markdown: {
    hooks: {
      onBrokenMarkdownLinks: "throw",
    },
  },
  favicon: "img/favicon.ico",
  organizationName: "convex", // Usually your GitHub org/user name.
  projectName: "Convex", // Usually your repo name.

  // Even if you don't use internalization, you can use this field to set useful
  // metadata like html lang. For example, if your site is Chinese, you may want
  // to replace "en" with "zh-Hans".
  i18n: {
    defaultLocale: "en",
    locales: ["en"],
  },
  customFields: {
    // Make these environment variables available to the docs site.
    KAPA_AI_PROJECT: process.env.KAPA_AI_PROJECT,
    KAPA_AI_KEY: process.env.KAPA_AI_KEY,
  },
  themeConfig: {
    colorMode: {
      respectPrefersColorScheme: true,
    },
    // Replace with your project's social card
    // image: "img/docusaurus-social-card.jpg", // TODO!
    docs: {
      sidebar: {
        hideable: false,
        autoCollapseCategories: true,
      },
    },
    navbar: {
      hideOnScroll: true,
      // If you change the Convex logo or the "docs" link, also update
      // src/theme/DocSidebar/Desktop/index.js to make sure the appearance
      // when the navbar disappears on scroll is consistent.
      logo: {
        href: "https://convex.dev",
        alt: "Convex",
        src: "img/convex-light.svg",
        srcDark: "img/convex-dark.svg",
      },
      items: [
        {
          // See the comment above if you're modifying this link
          type: "docSidebar",
          position: "left",
          sidebarId: "docs",
          html: `<svg class="convex-docs-title" width="53" height="37.5" viewBox="3 0 212 150" fill="none" xmlns="http://www.w3.org/2000/svg">
            <title>Docs</title>
            <path d="M27.1364 105.543C22.7727 105.543 18.9205 104.441 15.5795 102.236C12.2386 100.009 9.625 96.8726 7.73864 92.8271C5.85227 88.759 4.90909 83.9521 4.90909 78.4067C4.90909 72.9067 5.85227 68.134 7.73864 64.0885C9.625 60.0431 12.25 56.9181 15.6136 54.7135C18.9773 52.509 22.8636 51.4067 27.2727 51.4067C30.6818 51.4067 33.375 51.9749 35.3523 53.1112C37.3523 54.2249 38.875 55.4976 39.9205 56.9294C40.9886 58.3385 41.8182 59.4976 42.4091 60.4067H43.0909V34.634H51.1364V104.452H43.3636V96.4067H42.4091C41.8182 97.3612 40.9773 98.5658 39.8864 100.02C38.7955 101.452 37.2386 102.736 35.2159 103.873C33.1932 104.986 30.5 105.543 27.1364 105.543ZM28.2273 98.3158C31.4545 98.3158 34.1818 97.4749 36.4091 95.7931C38.6364 94.0885 40.3295 91.7362 41.4886 88.7362C42.6477 85.7135 43.2273 82.2249 43.2273 78.2703C43.2273 74.3612 42.6591 70.9408 41.5227 68.009C40.3864 65.0544 38.7045 62.759 36.4773 61.1226C34.25 59.4635 31.5 58.634 28.2273 58.634C24.8182 58.634 21.9773 59.509 19.7045 61.259C17.4545 62.9862 15.7614 65.3385 14.625 68.3158C13.5114 71.2703 12.9545 74.5885 12.9545 78.2703C12.9545 81.9976 13.5227 85.384 14.6591 88.4294C15.8182 91.4521 17.5227 93.8612 19.7727 95.6567C22.0455 97.4294 24.8636 98.3158 28.2273 98.3158ZM87.3014 105.543C82.5741 105.543 78.4264 104.418 74.8582 102.168C71.3127 99.9181 68.54 96.7703 66.54 92.7249C64.5627 88.6794 63.5741 83.9521 63.5741 78.5431C63.5741 73.0885 64.5627 68.3271 66.54 64.259C68.54 60.1908 71.3127 57.0317 74.8582 54.7817C78.4264 52.5317 82.5741 51.4067 87.3014 51.4067C92.0286 51.4067 96.165 52.5317 99.7105 54.7817C103.279 57.0317 106.051 60.1908 108.029 64.259C110.029 68.3271 111.029 73.0885 111.029 78.5431C111.029 83.9521 110.029 88.6794 108.029 92.7249C106.051 96.7703 103.279 99.9181 99.7105 102.168C96.165 104.418 92.0286 105.543 87.3014 105.543ZM87.3014 98.3158C90.8923 98.3158 93.8468 97.3953 96.165 95.5544C98.4832 93.7135 100.199 91.2931 101.313 88.2931C102.426 85.2931 102.983 82.0431 102.983 78.5431C102.983 75.0431 102.426 71.7817 101.313 68.759C100.199 65.7362 98.4832 63.2931 96.165 61.4294C93.8468 59.5658 90.8923 58.634 87.3014 58.634C83.7105 58.634 80.7559 59.5658 78.4377 61.4294C76.1195 63.2931 74.4036 65.7362 73.29 68.759C72.1764 71.7817 71.6195 75.0431 71.6195 78.5431C71.6195 82.0431 72.1764 85.2931 73.29 88.2931C74.4036 91.2931 76.1195 93.7135 78.4377 95.5544C80.7559 97.3953 83.7105 98.3158 87.3014 98.3158ZM143.623 105.543C138.714 105.543 134.486 104.384 130.941 102.066C127.395 99.7476 124.668 96.5544 122.759 92.4862C120.85 88.4181 119.895 83.7703 119.895 78.5431C119.895 73.2249 120.873 68.5317 122.827 64.4635C124.804 60.3726 127.554 57.1794 131.077 54.884C134.623 52.5658 138.759 51.4067 143.486 51.4067C147.168 51.4067 150.486 52.0885 153.441 53.4521C156.395 54.8158 158.816 56.7249 160.702 59.1794C162.589 61.634 163.759 64.4976 164.214 67.7703H156.168C155.554 65.384 154.191 63.2703 152.077 61.4294C149.986 59.5658 147.168 58.634 143.623 58.634C140.486 58.634 137.736 59.4521 135.373 61.0885C133.032 62.7021 131.202 64.9862 129.884 67.9408C128.589 70.8726 127.941 74.3158 127.941 78.2703C127.941 82.3158 128.577 85.8385 129.85 88.8385C131.145 91.8385 132.964 94.1681 135.304 95.8271C137.668 97.4862 140.441 98.3158 143.623 98.3158C145.714 98.3158 147.611 97.9521 149.316 97.2249C151.02 96.4976 152.464 95.4521 153.645 94.0885C154.827 92.7249 155.668 91.0885 156.168 89.1794H164.214C163.759 92.2703 162.634 95.0544 160.839 97.5317C159.066 99.9862 156.714 101.941 153.782 103.395C150.873 104.827 147.486 105.543 143.623 105.543ZM212.106 63.8158L204.879 65.8612C204.424 64.6567 203.754 63.4862 202.867 62.3499C202.004 61.1908 200.822 60.2362 199.322 59.4862C197.822 58.7362 195.901 58.3612 193.56 58.3612C190.356 58.3612 187.685 59.0999 185.549 60.5771C183.435 62.0317 182.379 63.884 182.379 66.134C182.379 68.134 183.106 69.7135 184.56 70.8726C186.015 72.0317 188.288 72.9976 191.379 73.7703L199.151 75.6794C203.833 76.8158 207.322 78.5544 209.617 80.8953C211.913 83.2135 213.06 86.2021 213.06 89.8612C213.06 92.8612 212.197 95.5431 210.469 97.9067C208.765 100.27 206.379 102.134 203.31 103.498C200.242 104.861 196.674 105.543 192.606 105.543C187.265 105.543 182.844 104.384 179.344 102.066C175.844 99.7476 173.629 96.3612 172.697 91.9067L180.333 89.9976C181.06 92.8158 182.435 94.9294 184.458 96.3385C186.504 97.7476 189.174 98.4521 192.469 98.4521C196.219 98.4521 199.197 97.6567 201.401 96.0658C203.629 94.4521 204.742 92.5203 204.742 90.2703C204.742 88.4521 204.106 86.9294 202.833 85.7021C201.56 84.4521 199.606 83.5203 196.969 82.9067L188.242 80.8612C183.447 79.7249 179.924 77.9635 177.674 75.5771C175.447 73.1681 174.333 70.1567 174.333 66.5431C174.333 63.5885 175.163 60.9749 176.822 58.7021C178.504 56.4294 180.788 54.6453 183.674 53.3499C186.583 52.0544 189.879 51.4067 193.56 51.4067C198.742 51.4067 202.81 52.5431 205.765 54.8158C208.742 57.0885 210.856 60.0885 212.106 63.8158Z" fill="currentColor"/>
          </svg>`,
        },
        {
          type: "custom-convex-search",
          position: "left",
        },
        {
          type: "custom-convex-ai-chat",
          position: "left",
        },
        {
          href: "https://dashboard.convex.dev",
          label: "Dashboard",
          position: "right",
          className: "convex-dashboard-button",
        },
        {
          // Using “to” instead of “href” to avoid Docusaurus adding a “external link” icon
          to: "https://stack.convex.dev/",
          label: "Blog",
          position: "right",
          className: "convex-blog-button",
        },
        {
          href: "https://github.com/convex",
          label: "GitHub",
          position: "right",
          className: "convex-github-logo convex-icon-link",
        },
        {
          href: "https://convex.dev/community",
          label: "Discord",
          position: "right",
          className: "convex-discord-logo convex-icon-link",
        },
      ],
    },
    footer: {
      links: [
        {
          href: "https://convex.dev/releases",
          label: "Releases",
        },
        {
          label: "GitHub",
          href: "https://github.com/convex",
          className: "convex-github-logo convex-icon-link",
        },
        {
          label: "Discord",
          to: "https://convex.dev/community",
          className: "convex-discord-logo convex-icon-link",
        },
        {
          label: "Twitter",
          href: "https://twitter.com/convex",
          className: "convex-twitter-logo convex-icon-link",
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Convex, Inc.`,
    },
    prism: {
      theme: lightCodeTheme,
      darkTheme: darkCodeTheme,
      additionalLanguages: ["rust", "kotlin", "swift", "bash", "shell-session"],
    },
    image: "img/social.png",
    metadata: [
      { name: "twitter:card", content: "summary_large_image" },
      { name: "twitter:image:alt", content: "Convex Docs logo" },
      { name: "og:image:alt", content: "Convex Docs logo" },
    ],
    languageTabs: [
      // We'll provide a typed client instead.
      /*
      {
        highlight: "javascript",
        language: "javascript",
        logoClass: "nodejs",
        variants: ["fetch"],
        options: {
          followRedirect: false,
        },
      },
      */
    ],
  } satisfies ThemeConfig,
  presets: [
    [
      "classic",
      {
        docs: {
          sidebarPath: resolve("./sidebars.js"),
          docItemComponent: "@theme/ApiItem",
          routeBasePath: "/",
          async sidebarItemsGenerator({
            defaultSidebarItemsGenerator,
            ...args
          }) {
            const originalSidebarItems =
              await defaultSidebarItemsGenerator(args);

            // Remove "API" and "Generated Code" from the main sidebar because
            // they have their own tab.
            if (
              args.item.type === "autogenerated" &&
              args.item.dirName === "."
            ) {
              const finalSidebarItems = originalSidebarItems.filter(
                (item) =>
                  !("label" in item) ||
                  (item.label !== "API" &&
                    item.label !== "Generated Code" &&
                    item.label !== "HTTP API"),
              );
              return finalSidebarItems;
            }

            // Drop `index.mdx` from "Generated Code" because it's already included
            // as the category index and Docusaurus is dumb and adds it twice.
            if (
              args.item.type === "autogenerated" &&
              args.item.dirName === "generated-api"
            ) {
              return originalSidebarItems.filter(
                (item) => !("id" in item) || item.id !== "generated-api/index",
              );
            }

            // If we have other autogenerated items, don't touch them.
            if (
              args.item.type === "autogenerated" &&
              args.item.dirName !== "api"
            ) {
              return originalSidebarItems;
            }

            /**
             * Custom generator for api sidebar items.
             *
             * We have a custom generator for the items in the sidebar because
             * we reorganize the API docs that docusaurus-plugin-typedoc generates.
             *
             * The original scheme is:
             * - API Reference
             *   - Modules
             *     - One item per entry point
             *   - Interfaces
             *     - All the interfaces for all the entry points
             *   - Classes
             *     - All the classes for all the entry points
             *
             * We reorganize that into:
             * - API Reference
             *   - convex/$entryPoint
             *     - classes, interfaces for $entrypoint
             *   - Generated Code
             *     - generated hooks and types.
             */
            const entryPointToItems = {};
            for (const entryPoint of ENTRY_POINTS_TO_DOCUMENT) {
              entryPointToItems[entryPoint] = [];
            }

            for (const category of originalSidebarItems) {
              // Skip the "Table of contents" category because we don't need
              // it, the "Modules" category because we create that ourselves
              // below, and "Readme" because it's already in sidebars.js.

              // The rest are things like "Classes" and "Interfaces" that we
              // want to reorganize.
              if (
                "items" in category &&
                (!("label" in category) ||
                  (category.label !== "Readme" &&
                    category.label !== "Table of contents" &&
                    category.label !== "modules"))
              ) {
                for (const item of category.items) {
                  if (!("id" in item)) {
                    continue;
                  }
                  // The original item ID looks like "api/classes/browser.ConvexHttpClient"
                  // and we want to extract "browser" because that's the entry point.
                  const pathParts = item.id.split("/");
                  const itemName = pathParts[pathParts.length - 1];
                  // Undo react-auth0 -> react_auth0 normalization.
                  const entryPoint = itemName.split(".")[0].replace("_", "-");
                  if (!ENTRY_POINTS_TO_DOCUMENT.includes(entryPoint)) {
                    throw new Error(
                      "Couldn't sort API reference doc by entry point: " +
                        item.id,
                    );
                  }

                  entryPointToItems[entryPoint].push({
                    ...item,
                    label: itemName.split(".")[1],
                  });
                }
              }
            }

            const entryPointCategories = ENTRY_POINTS_TO_DOCUMENT.map(
              (entryPoint) => {
                // Normalize the same way original sidebar items are.
                const entryPointForId = entryPoint.replace("-", "_");
                const items = entryPointToItems[entryPoint];
                const id = "api/modules/" + entryPointForId;
                const label = "convex/" + entryPoint;

                return items.length === 0
                  ? { type: "doc" as const, id, label }
                  : {
                      type: "category" as const,
                      label,
                      link: { type: "doc" as const, id },
                      items,
                    };
              },
            );
            return entryPointCategories;
          },
        },
        blog: {
          showReadingTime: true,
        },
        theme: {
          customCss: resolve("./src/css/custom.css"),
        },
      } satisfies PresetClassicOptions,
    ],
  ],
  plugins: [
    [
      "docusaurus-plugin-typedoc",
      {
        id: "api",
        entryPoints: ENTRY_POINTS_TO_DOCUMENT.map(
          (entryPoint) => "../convex/src/" + entryPoint + "/index.ts",
        ),
        tsconfig: "../convex/tsconfig.json",
        excludePrivate: true,
        excludeInternal: true,
        // Don't generate "defined in" text when generating docs because our
        // source isn't public.
        disableSources: false,
        sourceLinkTemplate:
          "https://github.com/convex/convex-js/blob/main/{path}#L{line}",
        gitRemote: "https://github.com/convex/convex-js",
        basePath: "../convex/src",
        // Keep everything in source order so we can be intentional about our
        // ordering. This seems to only work for functions, variables and type
        // aliases but it's something.
        sort: "source-order",
        out: "api",
        sidebar: {
          // Don't generate sidebar_label so we can always define it ourselves
          autoConfiguration: false,
        },
      },
    ],
    [
      "@signalwire/docusaurus-plugin-llms-txt",
      {
        siteTitle: "Convex Documentation",
        siteDescription:
          "For general information about Convex, read [https://www.convex.dev/llms.txt](https://www.convex.dev/llms.txt).",
        content: {
          enableLlmsFullTxt: true,
          excludeRoutes: [
            "/",
            "/home",
            "/quickstarts",
            "/understanding/best-practices/other-recommendations",
          ],
        },
        includeOrder: [
          "/understanding/**",
          "/quickstart/**",

          "/functions/**",
          "/database/**",
          "/realtime/**",
          "/auth/**",
          "/scheduling/**",
          "/file-storage/**",
          "/search/**",
          "/components/**",

          "/ai/**",
          "/ai-gateway/**",
          "/agents/**",
          "/testing/**",
          "/linting/**",
          "/production/**",
          "/self-hosting/**",

          "/cli/**",
          "/client/**",
          "/dashboard/**",
          "/error/**",
          "/home/**",
          "/tutorial/**",

          "/api/**",
          "/generated-api/**",
          "/http-api/**",
        ],
        onRouteError: "throw",
      },
    ],
    [
      "docusaurus-plugin-openapi-docs",
      {
        id: "openapi", // plugin id
        docsPluginId: "classic", // configured for preset-classic
        config: {
          management: {
            specPath: ".openapi-filtered/management-openapi.json",
            outputDir: "docs/management-api",
            sidebarOptions: {
              groupPathsBy: "tag",
            },
            hideSendButton: false,
          } satisfies OpenApiPlugin.Options,
          publicDeployment: {
            specPath: ".openapi-filtered/public-deployment-openapi.json",
            outputDir: "docs/public-deployment-api",
            sidebarOptions: {
              groupPathsBy: "tag",
            },
            hideSendButton: false,
          } satisfies OpenApiPlugin.Options,
          deployment: {
            specPath: ".openapi-filtered/deployment-openapi.json",
            outputDir: "docs/deployment-api",
            sidebarOptions: {
              groupPathsBy: "tag",
            },
            hideSendButton: false,
          } satisfies OpenApiPlugin.Options,
        },
      },
    ],
    "./src/plugins/prefixIds",
    "./src/plugins/imageZoomPlugin",
    async function versionPlugin() {
      // Exposes a /api/version route used by isitout
      return {
        name: "version-plugin",
        async postBuild({ outDir }: { outDir: string }) {
          const sha = execSync("git rev-parse HEAD").toString().trim();
          mkdirSync(join(outDir, "api"), { recursive: true });
          writeFileSync(
            join(outDir, "api", "version"),
            JSON.stringify({ sha }),
          );
        },
      };
    },
    async function tailwindPlugin() {
      return {
        name: "docusaurus-tailwindcss",
        configurePostCss(postcssOptions) {
          // eslint-disable-next-line @typescript-eslint/no-require-imports
          postcssOptions.plugins.push(require("@tailwindcss/postcss"));
          // eslint-disable-next-line @typescript-eslint/no-require-imports
          postcssOptions.plugins.push(require("postcss-nested"));
          return postcssOptions;
        },
      };
    },
  ],
  themes: ["docusaurus-theme-openapi-docs"],
};

export default config;
