import { getDistinctId } from "@convex-internal/web-analytics";
import { useConsent } from "@convex-internal/web-analytics/react";
import { useEffect } from "react";

const KAPA_SCRIPT_ID = "kapa-widget-script";

declare global {
  interface Window {
    kapaSettings?: {
      user: {
        uniqueClientId?: string;
      };
    };
  }
}

function createKapaWidgetScript() {
  if (document.getElementById(KAPA_SCRIPT_ID)) {
    return;
  }

  const script = document.createElement("script");
  script.id = KAPA_SCRIPT_ID;
  script.src = "https://widget.kapa.ai/kapa-widget.bundle.js";
  script.async = true;
  script.setAttribute(
    "data-website-id",
    "a20c0988-f33e-452b-9174-5045a58b965d",
  );
  script.setAttribute("data-project-name", "Convex");
  script.setAttribute("data-project-color", "#141414");
  script.setAttribute(
    "data-project-logo",
    "https://img.stackshare.io/service/41143/default_f1d33b63d360437ba28c8ac981dd68d7d2478b22.png",
  );
  script.setAttribute("data-button-hide", "true");
  script.setAttribute("data-modal-override-open-class", "js-launch-kapa-ai");
  script.setAttribute("data-render-on-load", "true");

  // Cookie and fingerprinting off by default.
  script.setAttribute("data-user-analytics-cookie-enabled", "false");
  script.setAttribute("data-user-analytics-fingerprint-enabled", "false");

  // We have a consent banner, so Kapa should not show its own.
  script.setAttribute("data-consent-required", "false");

  document.body.appendChild(script);
}

function setKapaCookieEnabled(enabled: boolean) {
  const script = document.getElementById(KAPA_SCRIPT_ID);
  script?.setAttribute(
    "data-user-analytics-cookie-enabled",
    enabled.toString(),
  );
}

function setKapaClientId(distinctId: string | undefined) {
  window.kapaSettings = {
    ...window.kapaSettings,
    user: {
      ...window.kapaSettings?.user,
      uniqueClientId: distinctId,
    },
  };
}

export function useKapaWidget() {
  const { consent } = useConsent();

  // Create the Kapa widget script on mount (Ask AI works without consent).
  useEffect(() => {
    createKapaWidgetScript();
  }, []);

  // Note Kapa may ignore live attribute and kapaSettings changes until reload.
  // See: https://convexdev.slack.com/archives/C06P55VN8Q4/p1749147459001719
  useEffect(() => {
    if (consent === "consented") {
      setKapaCookieEnabled(true);
      // After the parent PostHog opt-in runs.
      queueMicrotask(() => {
        setKapaClientId(getDistinctId());
      });
      return;
    }

    // Without consent, make sure cookies are disabled.
    setKapaCookieEnabled(false);
    setKapaClientId(undefined);
  }, [consent]);
}
