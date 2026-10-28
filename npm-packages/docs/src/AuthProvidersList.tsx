import React from "react";
import ConvexLogo from "@site/static/img/logomark.svg";
import ClerkLogo from "@site/static/img/clerk-logo.svg";
import WorkOSLogo from "@site/static/img/workos-logo.svg";
import Auth0Logo from "@site/static/img/auth0-logo.svg";
import KeyIcon from "heroicons/24/outline/key.svg";
import { DocCardList } from "@site/src/QuickstartsList";

// Icon cards for the setup guide of each authentication provider that can be
// configured in `convex/auth.config.ts`, in the same style as the quickstarts.
export function AuthProvidersList() {
  return (
    <DocCardList
      items={[
        {
          icon: <ConvexLogo height={40} width={40} />,
          href: "/auth/convex-auth",
          docId: "auth/convex-auth",
          label: "Convex Auth",
        },
        {
          icon: <ClerkLogo height={40} width={40} />,
          href: "/auth/clerk",
          docId: "auth/clerk",
          label: "Clerk",
        },
        {
          icon: <WorkOSLogo height={40} width={40} />,
          href: "/auth/authkit/",
          docId: "auth/authkit/index",
          label: "WorkOS",
        },
        {
          icon: <Auth0Logo height={40} width={40} />,
          href: "/auth/auth0",
          docId: "auth/auth0",
          label: "Auth0",
        },
        {
          icon: <KeyIcon height={40} width={40} />,
          href: "/auth/advanced/custom-auth",
          docId: "auth/advanced/custom-auth",
          label: "Custom OIDC",
        },
        {
          icon: <KeyIcon height={40} width={40} />,
          href: "/auth/advanced/custom-jwt",
          docId: "auth/advanced/custom-jwt",
          label: "Custom JWT",
        },
      ]}
    />
  );
}
