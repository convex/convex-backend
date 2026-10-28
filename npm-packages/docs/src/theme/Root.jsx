import React, { useEffect } from "react";
import { WebAnalyticsProvider } from "@convex-internal/web-analytics/react";

import { Toaster } from "sonner";

import "@fontsource/inter/300.css";
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/inter/700.css";
import "@fontsource/inter/800.css";

function Root({ children }) {
  // Scroll the active sidebar item into view in case
  // it's below fold.
  useEffect(() => {
    document.querySelectorAll(".menu__link--active").forEach((activeLink) => {
      // scrollIntoViewIfNeeded works great so use
      // it by default (Chrome, Safari)
      if (activeLink.scrollIntoViewIfNeeded) {
        activeLink.scrollIntoViewIfNeeded?.();
      } else {
        // If we used block: "center" it would
        // shift the whole page after page load
        activeLink.scrollIntoView({
          behavior: "instant",
          block: "nearest",
        });
      }
    });
  }, []);

  return (
    <WebAnalyticsProvider>
      {children}
      <Toaster />
    </WebAnalyticsProvider>
  );
}

export default Root;
