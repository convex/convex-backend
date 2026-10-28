import { toast } from "@common/lib/utils";
import { Link } from "@ui/Link";
import React from "react";
import { captureException } from "@sentry/nextjs";

// Constants for toast display
export const TOAST_AFTER_BACKOFF_COUNT = 3;
export const OFFLINE_TOAST_ID = "check_convex_status";

/**
 * Creates the content for the offline status toast
 */
export const createOfflineToastContent = () => (
  <p>
    Something seems wrong. The dashboard will attempt to reconnect
    automatically. Check{" "}
    <Link href="https://status.convex.dev/" target="_blank">
      Convex status
    </Link>{" "}
    for details and updates.
  </p>
);

/**
 * Shows a toast notification for offline status
 * @param error Optional error to capture in Sentry
 */
export const showOfflineToast = (error?: Error) => {
  toast("error", createOfflineToastContent(), OFFLINE_TOAST_ID, false);

  // Log the error to Sentry if provided
  if (error) {
    captureException(error.message);
  }
};
