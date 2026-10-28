# Convex management APIs

A client wrapping the management APIs available at

https://api.convex.dev/v1/openapi.json

as well as the deployment APIs available on every Convex backend.

Give `createDeploymentClient` the `deploymentUrl` from the management API.
Deployments in US East (`us-east-1`) answer on `https://<name>.convex.cloud`.
Deployments in every other [region](https://docs.convex.dev/production/regions)
answer on `https://<name>.<region>.convex.cloud`.
