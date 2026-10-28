// Fetch a local deployment's admin key and URL. Cloud deployments are
// authenticated with the WorkOS session instead, so this is the only deployment
// auth request the dashboard makes.
export const localDeploymentAuth = async (
  deploymentName: string,
  authHeader: string,
): Promise<
  | { deploymentUrl: string; adminKey: string; ok: true }
  | { ok: false; errorMessage: string; errorCode: string }
> => {
  const resp = await fetch(
    `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/api/dashboard/local_deployments/${deploymentName}/auth`,
    {
      headers: { Authorization: authHeader },
    },
  );
  const data = await resp.json();
  if (!resp.ok) {
    return { ok: false, errorCode: data.code, errorMessage: data.message };
  }
  const { adminKey, deploymentUrl } = data;
  return { deploymentUrl, adminKey, ok: true };
};
