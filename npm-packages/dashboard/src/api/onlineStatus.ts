import { backoffWithJitter } from "@common/lib/utils";
import {
  TOAST_AFTER_BACKOFF_COUNT,
  showOfflineToast,
} from "./offlineNotification";

// Cache for online status to avoid multiple simultaneous checks
// Using let since we need to reassign this object
let isOnlineCache = {
  status: true, // Assume online by default
};

// Track the interval ID so we can clear it if needed
let statusCheckIntervalId: NodeJS.Timeout | null = null;

// Promise-based mutex to prevent concurrent checks
export let checkMutex: Promise<boolean> | null = null;

// Track retry count for backoff and toast display
let retryCount = 0;

/**
 * Checks if the application is online by making a request to the version endpoint
 * @returns Promise that resolves to true if online, false otherwise
 */
const checkIsOnline = async (): Promise<boolean> => {
  // If a check is already in progress, return the existing promise
  if (checkMutex !== null) {
    return checkMutex;
  }

  // Create a new promise for this check and store it in the mutex
  checkMutex = (async () => {
    try {
      // Make a request to the version endpoint -- we just want to know if Big Brain is online
      await fetch(`${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/version`, {
        method: "GET",
        mode: "no-cors",
        // Short timeout to avoid hanging
        signal: AbortSignal.timeout(5000),
      });

      // Reset retry count when back online
      if (!isOnlineCache.status) {
        retryCount = 0;
      }

      // Update cache
      isOnlineCache = {
        status: true,
      };
      return true;
    } catch (error) {
      // Update cache - mark as offline immediately on first failure
      isOnlineCache = {
        status: false, // Mark as offline immediately
      };

      // Increment retry count
      retryCount++;

      // Show toast after certain number of retries
      if (retryCount === TOAST_AFTER_BACKOFF_COUNT) {
        showOfflineToast(error instanceof Error ? error : undefined);
      }

      // Schedule the next check with backoff
      scheduleNextOfflineCheck();

      return isOnlineCache.status;
    } finally {
      // Release the mutex after a small delay to prevent immediate subsequent calls
      // This ensures any queued calls that happen right after will still be deduped
      setTimeout(() => {
        checkMutex = null;
      }, 50);
    }
  })();

  return checkMutex;
};

/**
 * Schedule the next status check with backoff jitter when offline
 */
const scheduleNextOfflineCheck = () => {
  // Clear any existing interval
  if (statusCheckIntervalId !== null) {
    clearTimeout(statusCheckIntervalId);
  }

  // Calculate backoff time based on retry count
  // Use (retryCount - 1) because the first retry should use a base value of 0
  const backoffTime = backoffWithJitter(Math.max(0, retryCount - 1));

  console.log(
    `Disconnected from Dashboard server, retrying in ${backoffTime}ms`,
  );

  // Schedule the next check
  statusCheckIntervalId = setTimeout(async () => {
    await checkIsOnline();
  }, backoffTime);
};

// Function to get the current online status

/**
 * Force an immediate check of online status
 * This is the main entry point for checking online status
 * @returns Promise that resolves to true if online, false otherwise
 */
export const forceCheckIsOnline = async (): Promise<boolean> => {
  const result = await checkIsOnline();
  return result;
};
