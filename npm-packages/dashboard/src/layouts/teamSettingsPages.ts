import type { FC } from "react";
import {
  CounterClockwiseClockIcon,
  PieChartIcon,
  GearIcon,
  IdCardIcon,
  Link2Icon,
  PaperPlaneIcon,
  PersonIcon,
} from "@radix-ui/react-icons";
import {
  CreditCardIcon,
  KeyIcon,
  LockClosedIcon,
} from "@heroicons/react/24/outline";

export type TeamSettingsPage =
  | "general"
  | "members"
  | "billing"
  | "usage"
  | "audit-log"
  | "referrals"
  | "access-tokens"
  | "applications"
  | "custom-roles"
  | "team-authentication";

// Shared between the team settings sidebar and the command palette so both
// surfaces show the same icon for a page.
export const TEAM_SETTINGS_PAGE_ICONS: Record<
  TeamSettingsPage,
  FC<{ className?: string }>
> = {
  general: GearIcon,
  members: PersonIcon,
  billing: CreditCardIcon,
  usage: PieChartIcon,
  // Same icon as the deployment History page: both are audit trails.
  "audit-log": CounterClockwiseClockIcon,
  referrals: PaperPlaneIcon,
  "access-tokens": KeyIcon,
  applications: Link2Icon,
  "custom-roles": IdCardIcon,
  "team-authentication": LockClosedIcon,
};
