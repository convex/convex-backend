import {
  createManagementClient,
  createDeploymentClient,
} from "@convex-dev/platform";
import { program } from "commander";

async function main({
  accessToken,
  teamId: requestedTeamId,
}: {
  accessToken: string;
  teamId?: number | undefined;
}) {
  if (!process.env.CONVEX_PROVISION_HOST) {
    throw new Error("Must set CONVEX_PROVISION_HOST");
  }
  console.log("created client with access token:", accessToken);
  const client = createManagementClient(accessToken);

  let teamId;
  if (requestedTeamId === undefined) {
    const tokenDetailsResponse = await client.GET("/token_details");
    if (tokenDetailsResponse.error || !tokenDetailsResponse.response.ok) {
      throw new Error(
        `Error getting token details: ${(tokenDetailsResponse as any).error}`,
      );
    }
    if (tokenDetailsResponse.data.type !== "teamToken") {
      throw new Error(
        `unexpected token type: ${tokenDetailsResponse.data.type}`,
      );
    }
    teamId = tokenDetailsResponse.data.teamId;
  } else {
    teamId = requestedTeamId;
  }

  const projectsResponse = await client.GET("/teams/{team_id}/projects", {
    params: {
      path: {
        team_id: teamId,
      },
    },
  });

  if (projectsResponse.error || !projectsResponse.response.ok) {
    throw new Error(
      `Error listing projects: ${(projectsResponse as any).error}`,
    );
  }

  console.log(`${projectsResponse.data.items.length} projects`);

  const createProjectResponse = await client.POST(
    "/teams/{team_id}/create_project",
    {
      params: {
        path: {
          team_id: teamId,
        },
      },
      body: {
        projectName: `Project created by API at ${Date.now()}`,
        deploymentType: "dev",
      },
    },
  );
  if (createProjectResponse.error || !createProjectResponse.response.ok) {
    throw new Error(
      "Error creating project:",
      (createProjectResponse as any).error,
    );
  }

  const { deploymentName, deploymentUrl } = createProjectResponse.data;
  if (!deploymentName || !deploymentUrl) {
    throw new Error(
      "/create_project has unexpectedly created no deployment when one was requested.",
    );
  }

  const createDeployKeyResponse = await client.POST(
    "/deployments/{deployment_name}/create_deploy_key",
    {
      params: {
        path: {
          deployment_name: deploymentName,
        },
      },
      body: {
        name: "Smoke Test Deploy Key",
      },
    },
  );
  if (createDeployKeyResponse.error || !createDeployKeyResponse.response.ok) {
    throw new Error(
      "Error creating deploy key:",
      (createDeployKeyResponse as any).error,
    );
  }
  console.log("Deploy key created successfully!");
  console.log(`  - Deploy Key: ${createDeployKeyResponse.data.deployKey}`);

  // Now let's use this deployment
  const deployment = createDeploymentClient(
    deploymentUrl,
    createDeployKeyResponse.data.deployKey,
  );
  console.log(
    deploymentName,
    deploymentUrl,
    createDeployKeyResponse.data.deployKey,
  );

  // Call a function that doesn't exist
  // (This doesn't require auth)
  const runResponse = await deployment.POST("/update_environment_variables", {
    body: {
      changes: [
        {
          name: "FOO",
          value: "BAR",
        },
      ],
    },
  });
  console.log(
    "Setting an environment variable returns",
    runResponse.response.status,
    runResponse.response.statusText,
  );
  if (!runResponse.response.ok) {
    throw new Error(
      `failed to update_environment_variables: ${runResponse.response.status} ${runResponse.response.statusText}`,
    );
  }

  const listResponse = await deployment.GET("/list_environment_variables", {});

  if (!runResponse.response.ok) {
    throw new Error(
      `failed to list_environment_variables: ${runResponse.response.status} ${runResponse.response.statusText}`,
    );
  }
  console.log("Listing environment variables returns", listResponse.data);
  const expected = JSON.stringify({ FOO: "BAR" });
  const actual = JSON.stringify(listResponse.data?.environmentVariables);
  if (actual !== expected) {
    throw new Error(
      "Got unexpected envvar response " + actual + " instead of " + expected,
    );
  }

  // Test canonical URLs
  console.log("Testing canonical URLs...");

  // Get initial default URLs before customization
  const initialCanonicalUrlsResponse = await deployment.GET(
    "/get_canonical_urls",
    {},
  );
  if (!initialCanonicalUrlsResponse.response.ok) {
    throw new Error(
      `failed to get initial canonical URLs: ${initialCanonicalUrlsResponse.response.status}`,
    );
  }
  const defaultCloudUrl = initialCanonicalUrlsResponse.data?.convexCloudUrl;
  const defaultSiteUrl = initialCanonicalUrlsResponse.data?.convexSiteUrl;
  console.log("Default URLs:", { defaultCloudUrl, defaultSiteUrl });

  const updateCanonicalUrlResponse = await deployment.POST(
    "/update_canonical_url",
    {
      body: {
        requestDestination: "convexCloud",
        url: "https://custom-cloud.example.com",
      },
    },
  );
  console.log(
    "Updating canonical URL returns",
    updateCanonicalUrlResponse.response.status,
    updateCanonicalUrlResponse.response.statusText,
  );
  if (!updateCanonicalUrlResponse.response.ok) {
    throw new Error(
      `failed to update_canonical_url: ${updateCanonicalUrlResponse.response.status} ${updateCanonicalUrlResponse.response.statusText}`,
    );
  }

  const listCanonicalUrlsResponse = await deployment.GET(
    "/get_canonical_urls",
    {},
  );
  if (!listCanonicalUrlsResponse.response.ok) {
    throw new Error(
      `failed to get_canonical_urls: ${listCanonicalUrlsResponse.response.status} ${listCanonicalUrlsResponse.response.statusText}`,
    );
  }
  console.log("Listing canonical URLs returns", listCanonicalUrlsResponse.data);
  if (
    listCanonicalUrlsResponse.data?.convexCloudUrl !==
    "https://custom-cloud.example.com"
  ) {
    throw new Error(
      "Got unexpected convexCloud URL: " +
        listCanonicalUrlsResponse.data?.convexCloudUrl,
    );
  }
  // When no custom URL is set for convexSite, should return the default URL
  if (listCanonicalUrlsResponse.data?.convexSiteUrl !== defaultSiteUrl) {
    throw new Error(
      `Expected convexSite to be default URL (${defaultSiteUrl}), got: ${listCanonicalUrlsResponse.data?.convexSiteUrl}`,
    );
  }

  // Delete canonical URL
  const deleteCanonicalUrlResponse = await deployment.POST(
    "/update_canonical_url",
    {
      body: {
        requestDestination: "convexCloud",
        url: null,
      },
    },
  );
  if (!deleteCanonicalUrlResponse.response.ok) {
    throw new Error(
      `failed to delete canonical URL: ${deleteCanonicalUrlResponse.response.status} ${deleteCanonicalUrlResponse.response.statusText}`,
    );
  }

  const listCanonicalUrlsAfterDeleteResponse = await deployment.GET(
    "/get_canonical_urls",
    {},
  );
  if (!listCanonicalUrlsAfterDeleteResponse.response.ok) {
    throw new Error(
      `failed to get_canonical_urls after delete: ${listCanonicalUrlsAfterDeleteResponse.response.status}`,
    );
  }
  // After deletion, both URLs should return to their default values (not undefined)
  if (
    listCanonicalUrlsAfterDeleteResponse.data?.convexCloudUrl !==
      defaultCloudUrl ||
    listCanonicalUrlsAfterDeleteResponse.data?.convexSiteUrl !== defaultSiteUrl
  ) {
    throw new Error(
      `After deleting custom URLs, should return to defaults (cloud: ${defaultCloudUrl}, site: ${defaultSiteUrl}). Got: ${JSON.stringify(listCanonicalUrlsAfterDeleteResponse.data)}`,
    );
  }
  console.log("Canonical URLs test passed!");

  console.log("deleting...");

  const deleteProjectResponse = await client.POST(
    "/projects/{project_id}/delete",
    {
      params: {
        path: {
          project_id: createProjectResponse.data.projectId,
        },
      },
    },
  );
  if (!deleteProjectResponse.error && deleteProjectResponse.response.ok) {
    console.log("deleted project.");
  } else {
    throw new Error(`failed to delete project ${deleteProjectResponse}`);
  }
}

program
  .argument("<accessToken>", "Access token for Convex API")
  .option("-t, --team-id <teamId>", "Team ID")
  .action((accessToken, options) => {
    main({
      accessToken,
      teamId:
        options.teamId === undefined ? undefined : parseInt(options.teamId),
    });
  });

program.parse();
