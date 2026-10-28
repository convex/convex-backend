import { Link } from "@ui/Link";
import React, { useContext } from "react";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { Sheet } from "@ui/Sheet";
import { CopyTextButton } from "@common/elements/CopyTextButton";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

export function AuthConfig() {
  const authProviders = useQuery(udfs.listAuthProviders.default);
  const { deploymentsURI, isSelfHosted } = useContext(DeploymentInfoContext);
  return (
    <div className="flex flex-col gap-2">
      <Sheet>
        <div>
          <h3 className="mb-4">Authentication Configuration</h3>
          {!authProviders || authProviders.length === 0 ? (
            <p className="my-6 flex w-full flex-col items-center gap-2 text-sm text-content-secondary">
              This deployment has no authentication providers yet.
              <Link
                passHref
                href="https://docs.convex.dev/auth"
                target="_blank"
                externalIcon
              >
                Learn more about authentication
              </Link>
            </p>
          ) : (
            <>
              <p className="mt-4 text-sm">
                These are the authentication providers configured for this
                deployment.
              </p>
              <div className="flex max-w-3xl flex-col divide-y divide-border-transparent">
                {authProviders?.map((provider, i) => (
                  <div key={i} className="flex flex-wrap gap-4 py-6">
                    {"type" in provider ? (
                      <>
                        <ProviderAttribute
                          label="Issuer"
                          value={provider.issuer}
                        />
                        <ProviderAttribute
                          label="JWKS URL"
                          value={provider.jwks}
                        />
                        <ProviderAttribute
                          label="Algorithm"
                          value={provider.algorithm.replace(/^"(.*)"$/, "$1")}
                        />
                        {provider.applicationID && (
                          <ProviderAttribute
                            label="Application ID"
                            value={provider.applicationID}
                          />
                        )}
                        <div className="flex flex-col gap-2 text-xs">
                          <span className="font-semibold">Type</span>
                          <Link
                            href="https://docs.convex.dev/auth/advanced/custom-jwt"
                            className="border-y border-transparent py-1 text-sm font-normal"
                            target="_blank"
                          >
                            Custom JWT provider
                          </Link>
                        </div>
                      </>
                    ) : (
                      <>
                        <ProviderAttribute
                          label="Domain"
                          value={provider.domain}
                        />
                        <ProviderAttribute
                          label="Application ID"
                          value={provider.applicationID}
                        />
                        <div className="flex flex-col gap-2 text-xs">
                          <span className="font-semibold">Type</span>
                          <Link
                            href="https://docs.convex.dev/auth/advanced/custom-auth"
                            className="border-y border-transparent py-1 text-sm font-normal"
                            target="_blank"
                          >
                            OIDC provider
                          </Link>
                        </div>
                      </>
                    )}
                  </div>
                ))}
              </div>
            </>
          )}
        </div>
      </Sheet>
      {!isSelfHosted && (
        <p className="text-sm text-content-secondary">
          Looking to create a Deploy Key? You can do so in{" "}
          <Link href={`${deploymentsURI}/settings`}>
            General Deployment Settings
          </Link>
          .
        </p>
      )}
    </div>
  );
}

export function ProviderAttribute({
  label,
  value,
}: {
  label: string;
  value: string;
}) {
  return (
    <div className="flex max-w-96 flex-col gap-2 text-xs">
      <span className="font-semibold">{label}</span>
      <CopyTextButton text={value} className="truncate text-sm font-normal" />
    </div>
  );
}
