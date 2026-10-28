import { useState } from "react";
import { Link } from "@ui/Link";
import { Button } from "@ui/Button";
import { toast } from "@common/lib/utils";
import { CopyIcon } from "@radix-ui/react-icons";

interface WorkOSCredentialsSectionProps {
  clientId: string;
  apiKey?: string;
  isProduction: boolean;
  teamSlug?: string;
}

export function WorkOSCredentialsSection({
  clientId,
  apiKey,
  isProduction,
  teamSlug,
}: WorkOSCredentialsSectionProps) {
  const [showCookiePassword, setShowCookiePassword] = useState(false);
  const [cookiePassword, setCookiePassword] = useState<string>("");

  // Generate a random 32-byte string when checkbox is toggled
  const generateCookiePassword = () => {
    const chars =
      "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const array = new Uint32Array(32);
    crypto.getRandomValues(array);
    return Array.from(array, (n) => chars[n % chars.length]).join("");
  };

  const handleToggleCookiePassword = (checked: boolean) => {
    setShowCookiePassword(checked);
    if (checked) {
      // Always generate a new cookie password when toggled on
      setCookiePassword(generateCookiePassword());
    }
  };

  const handleCopyAll = () => {
    const vars = [
      `WORKOS_CLIENT_ID="${clientId}"`,
      apiKey && `WORKOS_API_KEY="${apiKey}"`,
      showCookiePassword &&
        cookiePassword &&
        `WORKOS_COOKIE_PASSWORD="${cookiePassword}"`,
    ]
      .filter(Boolean)
      .join("\n");

    void navigator.clipboard.writeText(vars);
    toast("success", "Environment variables copied to clipboard");
  };

  return (
    <div className="rounded-sm border bg-background-secondary">
      <div className="px-4 py-3">
        <p className="mb-3 text-xs text-content-secondary">
          Copy WORKOS_* environment variables to your build environment (like
          Vercel) to set up AuthKit for your frontend and run{" "}
          <Link href="https://docs.convex.dev/auth/authkit">
            authKit configuration specified in convex.json
          </Link>{" "}
          during builds.
        </p>

        <div className="space-y-2">
          {/* Basic credentials with copy button */}
          <div className="relative">
            <div className="overflow-hidden rounded-sm border bg-background-primary">
              <div className="flex items-center justify-between border-b bg-background-secondary px-3 py-2">
                <span className="text-xs font-medium text-content-secondary">
                  Environment Variables
                </span>
                <Button
                  size="xs"
                  variant="neutral"
                  onClick={handleCopyAll}
                  icon={<CopyIcon className="size-3" />}
                >
                  Copy All
                </Button>
              </div>
              <div className="p-3">
                <pre className="font-mono text-xs break-all whitespace-pre-wrap">
                  {`WORKOS_CLIENT_ID="${clientId}"`}
                  {apiKey &&
                    `
WORKOS_API_KEY="${apiKey}"`}
                  {showCookiePassword &&
                    cookiePassword &&
                    `
WORKOS_COOKIE_PASSWORD="${cookiePassword}"`}
                </pre>
              </div>
            </div>
          </div>

          {/* Cookie password checkbox */}
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="show-cookie-password"
              checked={showCookiePassword}
              onChange={(e) => handleToggleCookiePassword(e.target.checked)}
              className="size-3.5"
            />
            <label
              htmlFor="show-cookie-password"
              className="cursor-pointer text-xs text-content-secondary"
            >
              Add random WORKOS_COOKIE_PASSWORD{" "}
              <span className="text-xs text-content-tertiary">(not saved)</span>
            </label>
          </div>

          {/* Deploy key note */}
          {teamSlug && (
            <div className="pt-2 text-xs text-content-tertiary">
              Note: For CI/CD, you'll also need a deployment key.
              {isProduction ? (
                <>
                  {" "}
                  Create one in{" "}
                  <Link href={`/t/${teamSlug}/settings/deploy-keys#production`}>
                    Team Settings → Production Deploy Keys
                  </Link>
                </>
              ) : (
                <>
                  {" "}
                  Create one in{" "}
                  <Link href={`/t/${teamSlug}/settings/deploy-keys#preview`}>
                    Team Settings → Preview Deploy Keys
                  </Link>
                </>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
