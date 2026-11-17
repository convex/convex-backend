import React from "react";
import clsx from "clsx";
import Link from "@docusaurus/Link";

const sectionLabels = [
  ["/tutorial/", "Tutorial"],
  ["/quickstart/", "Quickstarts"],
  ["/understanding/", "Understand Convex"],
  ["/functions/", "Functions"],
  ["/database/", "Database"],
  ["/realtime", "Realtime"],
  ["/auth/", "Authentication"],
  ["/scheduling/", "Scheduling"],
  ["/file-storage/", "File Storage"],
  ["/search/", "Search"],
  ["/components/", "Components"],
  ["/ai/", "AI Code Gen"],
  ["/agents/", "Agents"],
  ["/testing/", "Testing"],
  ["/production/", "Production"],
  ["/self-hosting", "Self Hosting"],
  ["/platform-apis/", "Platform APIs"],
  ["/client/react-native", "React Native"],
  ["/client/react/", "React"],
  ["/client/nextjs/", "Next.js"],
  ["/client/tanstack/", "TanStack"],
  ["/client/javascript/", "JavaScript"],
  ["/client/vue/", "Vue"],
  ["/client/svelte/", "Svelte"],
  ["/client/swift/", "Swift"],
  ["/client/android/", "Android"],
  ["/client/python", "Python"],
  ["/client/rust", "Rust"],
  ["/dashboard/", "Dashboard"],
  ["/cli/", "CLI"],
  ["/team-management/", "Team Management"],
  ["/generated-api/", "Generated API"],
  ["/http-api/", "HTTP API"],
  ["/deployment-api", "Deployment API"],
  ["/deployment-platform-api", "Deployment API"],
  ["/management-api", "Management API"],
  ["/home", "Home"],
];

function getSectionLabel(permalink) {
  return sectionLabels.find(([prefix]) => permalink.startsWith(prefix))?.[1];
}

function Arrow({ isNext }) {
  return (
    <svg
      className="pagination-nav__arrow"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={isNext ? "M6 3l5 5-5 5" : "M10 3L5 8l5 5"} />
    </svg>
  );
}

export default function PaginatorNavLink(props) {
  const { permalink, title, subLabel, isNext } = props;
  const sectionLabel = getSectionLabel(permalink);

  return (
    <Link
      className={clsx(
        "pagination-nav__link",
        isNext ? "pagination-nav__link--next" : "pagination-nav__link--prev",
      )}
      to={permalink}
    >
      <Arrow isNext={isNext} />
      <div className="pagination-nav__text">
        {/* The arrow conveys direction visually, so "Previous"/"Next" is only
            shown when there's no section label to take its place. */}
        {subLabel && (
          <div
            className={clsx(
              "pagination-nav__sublabel",
              sectionLabel && "sr-only",
            )}
          >
            {subLabel}
          </div>
        )}
        {sectionLabel && (
          <div className="pagination-nav__section-label">{sectionLabel}</div>
        )}
        <div className="pagination-nav__label">{title}</div>
      </div>
    </Link>
  );
}
