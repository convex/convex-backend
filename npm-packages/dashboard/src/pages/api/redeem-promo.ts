import { captureException } from "@sentry/nextjs";
import type { NextApiRequest, NextApiResponse } from "next";
import { getSession } from "server/workos";
import { z } from "zod";

type ResponseData =
  | {
      redeemed: true;
      team_id: number;
      credit_amount: number;
      description: string;
      orb_ledger_entry_id: string;
    }
  | { error: string; code?: string };

const RequestBodySchema = z.object({
  code: z.string().trim().min(1).max(1024),
  teamId: z.number().int(),
});
const PROMO_REDEMPTION_TIMEOUT_MS = 30_000;

export default async function handler(
  req: NextApiRequest,
  res: NextApiResponse<ResponseData>,
) {
  if (req.method !== "POST") {
    return res.status(405).json({ error: "Method not allowed" });
  }

  const session = await getSession(req);
  if (!session?.accessToken) {
    return res.status(401).json({ error: "Unauthorized" });
  }

  const parsedBody = RequestBodySchema.safeParse(req.body);
  if (!parsedBody.success) {
    return res.status(400).json({ error: "Enter a valid promo code." });
  }

  const { code, teamId } = parsedBody.data;
  try {
    const promoResponse = await fetch(`${process.env.PROMOS_URL}/redeem`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        authorization: `Bearer ${session.accessToken}`,
      },
      body: JSON.stringify({ code, team_id: teamId }),
      signal: AbortSignal.timeout(PROMO_REDEMPTION_TIMEOUT_MS),
    });
    const responseBody = (await promoResponse.json()) as ResponseData;
    return res.status(promoResponse.status).json(responseBody);
  } catch (error) {
    captureException(error);
    return res.status(502).json({
      error: "Unable to redeem the promo code. Please try again.",
    });
  }
}
