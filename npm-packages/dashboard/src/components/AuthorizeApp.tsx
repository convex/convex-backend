import { useTeams } from "api/teams";
import { usePaginatedProjects, useProjectById } from "api/projects";
import Head from "next/head";
import { Button } from "@ui/Button";
import { Combobox, MAX_DISPLAYED_OPTIONS } from "@ui/Combobox";
import { Loading } from "@ui/Loading";
import { useState, useEffect, useMemo } from "react";
import { useFormik } from "formik";
import { useDebounce } from "react-use";
import { useAccessToken } from "hooks/useServerSideData";
import { useRouter } from "next/router";
import { useAuthorizeApp } from "api/accessTokens";
import { useCheckOauthApp } from "api/oauth";
import { LoginLayout } from "layouts/LoginLayout";
import { Sheet } from "@ui/Sheet";
import { PlusIcon, ResetIcon } from "@radix-ui/react-icons";
import { CreateProjectForm } from "hooks/useCreateProjectModal";
import { Callout } from "@ui/Callout";
import { HelpTooltip } from "@ui/HelpTooltip";
import { captureException } from "@sentry/nextjs";
import { OauthAppResponse } from "generatedApi";

type AuthorizationScope = "project" | "team";

interface AuthorizeAppProps {
  authorizationScope: AuthorizationScope;
}

export function AuthorizeApp({ authorizationScope }: AuthorizeAppProps) {
  const router = useRouter();
  const [showProjectForm, setShowProjectForm] = useState(false);
  const [isRedirecting, setIsRedirecting] = useState(false);

  // oauth2 authorization code flow validation
  const oauthConfig: OAuthConfig = useMemo(
    () => ({
      clientId: router.query.client_id as string,
      redirectUri: router.query.redirect_uri as string,
      state: router.query.state as string | undefined,
      responseType: router.query.response_type as string | undefined,
      codeChallenge: router.query.code_challenge as string | undefined,
      codeChallengeMethod: router.query.code_challenge_method as
        | string
        | undefined,
    }),
    [router.query],
  );

  const { selectedTeamSlug, teams } = useTeams();
  const team = teams?.find((t) => t.slug === selectedTeamSlug) ?? undefined;

  // Check OAuth app using the new endpoint
  const checkOauthApp = useCheckOauthApp(team?.id);
  const [oauthAppData, setOauthAppData] = useState<OauthAppResponse | null>(
    null,
  );
  const [oauthError, setOauthError] = useState<string | null>(null);
  const [authorizeError, setAuthorizeError] = useState<string | null>(null);

  // Validate OAuth app when team is selected and we have the required parameters
  useEffect(() => {
    async function check() {
      if (!team?.id || !oauthConfig.clientId || !oauthConfig.redirectUri) {
        return;
      }

      try {
        const response = await checkOauthApp({
          clientId: oauthConfig.clientId,
          redirectUri: oauthConfig.redirectUri,
        });
        setOauthAppData(response);
        setOauthError(null);
      } catch (error: any) {
        setOauthAppData(null);
        setOauthError(error?.message || "Failed to validate OAuth app");
      }
    }
    void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [team?.id, oauthConfig.clientId, oauthConfig.redirectUri]);

  // Validate OAuth config
  const { validatedConfig, error } = validateOAuthConfig(oauthConfig);

  // Project selection logic (only used for project scope)
  const {
    projects,
    selectedProject,
    selectedProjectId,
    setSelectedProjectId,
    projectQuery,
    setProjectQuery,
    isLoading: projectsLoading,
  } = useProjectSelection(team);
  const [didCreateProject, setDidCreateProject] = useState(false);

  // Detect duplicate project names and prepare options
  const projectOptions = useMemo(() => {
    const nameCountMap = new Map<string, number>();
    projects?.forEach((p) => {
      nameCountMap.set(p.name, (nameCountMap.get(p.name) || 0) + 1);
    });

    const options =
      projects?.map((p) => {
        const isDuplicate = (nameCountMap.get(p.name) || 0) > 1;
        const label = isDuplicate && p.slug ? `${p.name} (${p.slug})` : p.name;
        return { label, value: p.id };
      }) ?? [];

    // Ensure the selected project is always an option, even when it's filtered
    // out of the paginated list, so its label survives in the button and
    // blurring the combobox doesn't deselect it.
    if (
      selectedProject &&
      !options.some((o) => o.value === selectedProject.id)
    ) {
      options.push({ label: selectedProject.name, value: selectedProject.id });
    }

    return options;
  }, [projects, selectedProject]);

  // Auto-show create project form if team has no projects
  useEffect(() => {
    if (!projectsLoading && projects?.length === 0 && projectQuery === "") {
      setShowProjectForm(true);
    }
  }, [projectsLoading, projects?.length, projectQuery]);

  const [accessToken] = useAccessToken();
  const authorizeApp = useAuthorizeApp();

  const formState = useFormik({
    initialValues: {},
    onSubmit: async () => {
      if (isRedirecting) {
        return;
      }

      try {
        let resp;

        if (authorizationScope === "team") {
          if (!team || !validatedConfig?.redirectUri) {
            throw new Error("Missing team or redirect URI");
          }

          resp = await authorizeApp({
            authnToken: accessToken,
            teamId: team.id, // Team-level auth
            clientId: validatedConfig.clientId,
            redirectUri: validatedConfig.redirectUri,
            codeChallenge: validatedConfig.codeChallenge,
            mode: "AuthorizationCode",
          });
        } else {
          // Project scope
          if (!selectedProjectId || !validatedConfig?.redirectUri) {
            throw new Error("Missing project or redirect URI");
          }

          const project = projects?.find((p) => p.id === selectedProjectId)!;
          resp = await authorizeApp({
            authnToken: accessToken,
            projectId: project.id,
            clientId: validatedConfig.clientId,
            redirectUri: validatedConfig.redirectUri,
            codeChallenge: validatedConfig.codeChallenge,
            mode: "AuthorizationCode",
          });
        }

        const redirectUrl = buildOAuthRedirectUrl(validatedConfig, {
          code: resp.code,
          state: validatedConfig?.state,
        });
        setAuthorizeError(null); // Clear any previous errors
        setIsRedirecting(true);
        void router.replace(redirectUrl);
      } catch (e: any) {
        setAuthorizeError(e?.message || "Failed to authorize application");
      }
    },
  });

  // Check for missing required parameters
  if (
    !oauthConfig.clientId ||
    !oauthConfig.redirectUri ||
    !oauthConfig.responseType ||
    oauthConfig.responseType !== "code"
  ) {
    return (
      <div className="h-screen">
        <Head>
          <title>
            Authorize Convex{" "}
            {authorizationScope === "team" ? "Team" : "Project"} Access
          </title>
        </Head>
        <LoginLayout>
          <Sheet className="flex max-w-prose flex-col gap-4 sm:min-w-lg">
            <h3>Authorize access to your {authorizationScope}</h3>
            <Callout variant="error">
              <div>
                Missing required OAuth parameters.
                <ul className="list-disc pl-4">
                  {!oauthConfig.clientId && (
                    <li>
                      <code>client_id</code> is required
                    </li>
                  )}
                  {!oauthConfig.redirectUri && (
                    <li>
                      <code>redirect_uri</code> is required
                    </li>
                  )}
                  {(!oauthConfig.responseType ||
                    oauthConfig.responseType !== "code") && (
                    <li>
                      <code>response_type</code> must be set to "code"
                    </li>
                  )}
                </ul>
                <p className="mt-2">
                  Contact the developer of the application that provided this
                  URL to you.
                </p>
              </div>
            </Callout>
          </Sheet>
        </LoginLayout>
      </div>
    );
  }

  // Handle any errors (OAuth app validation, config errors, or authorization errors)
  const currentError = oauthError || authorizeError || error;

  if (currentError) {
    if (isRedirecting) {
      return null;
    }

    // Always show the error on the page first, don't redirect immediately
    captureException(new Error(currentError));
    return (
      <div className="h-screen">
        <Head>
          <title>
            Authorize Convex{" "}
            {authorizationScope === "team" ? "Team" : "Project"} Access
          </title>
        </Head>
        <LoginLayout>
          <Sheet className="flex max-w-prose flex-col gap-4 sm:min-w-lg">
            <h3>Authorize access to your {authorizationScope}</h3>
            <Callout variant="error">
              <div>
                {currentError}
                <p className="mt-2">
                  Contact the developer of the application that provided this
                  URL to you.
                </p>
              </div>
            </Callout>
          </Sheet>
        </LoginLayout>
      </div>
    );
  }

  const renderTeamSelection = () => (
    <div className="flex flex-col gap-1">
      <Combobox
        labelHidden={false}
        options={
          teams?.map((t) => ({
            label: t.name,
            value: t.slug,
          })) ?? []
        }
        label={
          authorizationScope === "team" ? (
            <div className="flex items-center gap-1">
              <span>Select a team</span>
              <HelpTooltip>
                {`${oauthAppData?.appName} will only be able to operate within the selected team.`}
              </HelpTooltip>
            </div>
          ) : (
            "Select a team"
          )
        }
        selectedOption={selectedTeamSlug}
        setSelectedOption={(slug: string | null) => {
          if (slug !== null) {
            const searchParams = new URLSearchParams(window.location.search);
            searchParams.set("team", slug);
            void router.push(`?${searchParams.toString()}`);
          }
        }}
      />
    </div>
  );

  const renderProjectSelection = () => {
    if (showProjectForm) {
      return (
        <div className="flex gap-2">
          <CreateProjectForm
            onClose={() => {
              setShowProjectForm(false);
            }}
            team={team!}
            showLabel
            onSuccess={(project) => {
              setSelectedProjectId(project.projectId);
              setShowProjectForm(false);
              setDidCreateProject(true);
            }}
          />
          <Button
            variant="neutral"
            onClick={() => setShowProjectForm(false)}
            inline
            className="mt-7 h-fit"
            tip="Go back to selecting a project"
            tipSide="right"
            icon={<ResetIcon />}
          />
        </div>
      );
    }

    return (
      <div className="flex flex-wrap items-end gap-2">
        <div className="flex flex-col gap-1">
          <Combobox
            label={
              <div className="flex items-center gap-1">
                <span>Select a project</span>
                <HelpTooltip>
                  {`${oauthAppData?.appName} will only be able to operate within the selected project.`}
                </HelpTooltip>
              </div>
            }
            labelHidden={false}
            options={projectOptions}
            allowCustomValue={false}
            selectedOption={selectedProjectId}
            onFilterChange={setProjectQuery}
            isLoadingOptions={projectsLoading && projectQuery !== ""}
            setSelectedOption={(id) => {
              setSelectedProjectId(id as number | null);
            }}
            unknownLabel={() => "projects"}
          />
        </div>
        {!didCreateProject && (
          <div className="flex items-center gap-2">
            <span className="text-sm text-content-secondary">or</span>
            <Button
              variant="neutral"
              onClick={() => {
                setShowProjectForm(true);
                setSelectedProjectId(null);
              }}
              icon={<PlusIcon className="size-4" />}
            >
              Create a new project
            </Button>
          </div>
        )}
      </div>
    );
  };

  const renderAuthorizationDescription = () => {
    if (authorizationScope === "team") {
      return (
        <div className="flex flex-col gap-2">
          <p>
            Authorizing will allow{" "}
            <span className="font-semibold">{oauthAppData?.appName}</span> to:
          </p>
          <ul className="list-disc pl-4">
            <li>Create new projects</li>
            <li>Create new deployments</li>
            <li>
              <span className="flex items-center gap-1">
                Manage all projects on the selected team
                <HelpTooltip>
                  This includes actions like deleting projects, managing custom
                  domains, managing project environment variable defaults, and
                  managing cloud backups and restores.
                </HelpTooltip>
              </span>
            </li>
            <li>
              <span className="flex items-center gap-1">
                Read and write data in all projects on the selected team
                <HelpTooltip>
                  Write access to Production deployments will depend on your
                  team-level and project-level roles.
                </HelpTooltip>
              </span>
            </li>
          </ul>
        </div>
      );
    }

    return (
      <div>
        <p>
          Authorizing will give{" "}
          <span className="font-semibold">{oauthAppData?.appName}</span> access
          to:
        </p>
        <ul className="list-disc pl-4">
          <li>Create new deployments in the selected project</li>
          <li>
            <span className="flex items-center gap-1">
              Manage the selected project project
              <HelpTooltip>
                This includes actions like managing custom domains, managing
                environment variable defaults, and managing cloud backups and
                restores.
              </HelpTooltip>
            </span>
          </li>
          <li>
            <span className="flex items-center gap-1">
              Read and write data in any deployment in this project
              <HelpTooltip>
                Write access to Production deployments will depend on your
                team-level and project-level roles.
              </HelpTooltip>
            </span>
          </li>
        </ul>
      </div>
    );
  };

  const isFormValid = () => {
    if (authorizationScope === "team") {
      return selectedTeamSlug !== null;
    }
    return selectedProjectId !== null;
  };

  const getFormValidationTip = () => {
    if (authorizationScope === "team") {
      return !selectedTeamSlug ? "Select a team to continue" : undefined;
    }
    return !selectedProjectId
      ? "Select or create a project to continue"
      : undefined;
  };

  return (
    <div className="h-screen">
      <Head>
        <title>
          Authorize Convex {authorizationScope === "team" ? "Team" : "Project"}{" "}
          Access
        </title>
      </Head>
      <LoginLayout>
        <Sheet className="flex max-w-prose flex-col gap-4 sm:min-w-lg">
          <h3>Authorize access to your {authorizationScope}</h3>
          {!oauthAppData && !oauthError ? (
            <Loading className="h-80 w-full items-center justify-center" />
          ) : (
            <>
              {renderAuthorizationDescription()}
              <div className="flex flex-col gap-4">
                {renderTeamSelection()}
                {authorizationScope === "project" && renderProjectSelection()}
                <Button
                  variant="primary"
                  className="mt-2 ml-auto"
                  onClick={() => formState.handleSubmit()}
                  tip={getFormValidationTip()}
                  disabled={!isFormValid() || isRedirecting}
                  loading={formState.isSubmitting}
                >
                  Authorize
                </Button>
              </div>
            </>
          )}
        </Sheet>
      </LoginLayout>
    </div>
  );
}

// from RFC 6749 section 4.1.2.1
type OAuthError =
  | "invalid_request"
  | "unauthorized_client"
  | "access_denied"
  | "unsupported_response_type"
  | "invalid_scope"
  | "server_error"
  | "temporarily_unavailable";

export interface OAuthConfig {
  clientId: string;
  redirectUri: string;
  state?: string;
  responseType?: string;
  codeChallenge?: string;
  codeChallengeMethod?: string;
}

export interface ValidatedOAuthConfig {
  clientId: string;
  redirectUri: string;
  state?: string;
  responseType?: "code"; // Only support authorization code flow
  codeChallenge?: string;
}

export function validateOAuthConfig(config: OAuthConfig): {
  validatedConfig?: ValidatedOAuthConfig;
  error?: OAuthError;
  errorDescription?: string;
} {
  const validatedConfig: ValidatedOAuthConfig = {
    clientId: config.clientId,
    redirectUri: config.redirectUri,
    state: config.state,
  };

  // Only support authorization code flow
  if (config.responseType !== "code") {
    return {
      validatedConfig,
      error: "unsupported_response_type",
    };
  }
  validatedConfig.responseType = config.responseType;

  if (config.codeChallenge) {
    if (config.codeChallengeMethod !== "S256") {
      return {
        validatedConfig,
        error: "invalid_request",
        errorDescription: "unsupported code_challenge_method",
      };
    }
    if (config.codeChallenge.length !== 43) {
      return {
        validatedConfig,
        error: "invalid_request",
        errorDescription: "code_challenge is the wrong length for S256",
      };
    }
    validatedConfig.codeChallenge = config.codeChallenge;
  }

  return {
    validatedConfig,
  };
}

export function buildOAuthRedirectUrl(
  validatedConfig: ValidatedOAuthConfig | undefined,
  params: {
    error?: OAuthError;
    errorDescription?: string;
    code?: string;
    state?: string;
  },
): string {
  const redirectUri = validatedConfig?.redirectUri;

  // If no redirectUri was provided, we can't redirect anywhere
  if (!redirectUri) {
    throw new Error("redirectUri is missing");
  }

  const url = new URL(redirectUri);
  const queryParams: string[] = [];

  if (params.error) {
    queryParams.push(`error=${encodeURIComponent(params.error)}`);
    if (params.errorDescription) {
      queryParams.push(
        `error_description=${encodeURIComponent(params.errorDescription)}`,
      );
    }
  } else if (params.code) {
    queryParams.push(`code=${encodeURIComponent(params.code)}`);
  }

  if (params.state) {
    queryParams.push(`state=${encodeURIComponent(params.state)}`);
  }

  const responseParams = queryParams.join("&");
  url.search = responseParams;
  return url.toString();
}

function useProjectSelection(team?: { id: number }) {
  const [selectedProjectId, setSelectedProjectId] = useState<number | null>(
    null,
  );
  const [projectQuery, setProjectQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");

  // Debounce search query (300ms delay)
  useDebounce(
    () => {
      setDebouncedQuery(projectQuery);
    },
    300,
    [projectQuery],
  );

  // Fetch paginated projects with debounced query
  const paginatedData = usePaginatedProjects(team?.id, {
    q: debouncedQuery,
    limitOverride: MAX_DISPLAYED_OPTIONS,
  });

  const projects = paginatedData ? paginatedData.items : undefined;
  const isLoading = paginatedData === undefined;

  // Fetch the selected project directly so its label survives even when it's
  // filtered out of the paginated list.
  const { project: selectedProject } = useProjectById(
    selectedProjectId ?? undefined,
  );

  return {
    projects,
    selectedProject: selectedProject ?? null,
    selectedProjectId,
    setSelectedProjectId,
    projectQuery,
    setProjectQuery,
    isLoading,
  };
}
