import { captureException } from "@sentry/nextjs";
import type { NextApiRequest, NextApiResponse } from "next";
import { getSession } from "server/workos";
import { z } from "zod";

type ResponseData =
  | {
      code: string;
      description: string;
      credit_amount: number;
      expiration_time: number;
      credit_validity_days: number;
    }
  | { error: string; code?: string };

const RequestBodySchema = z.object({
  code: z.string().trim().min(1).max(1024),
});
const PROMO_LOOKUP_TIMEOUT_MS = 30_000;

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

  try {
    const promoResponse = await fetch(`${process.env.PROMOS_URL}/lookup`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        authorization: `Bearer ${session.accessToken}`,
      },
      body: JSON.stringify({ code: parsedBody.data.code }),
      signal: AbortSignal.timeout(PROMO_LOOKUP_TIMEOUT_MS),
    });
    const responseBody = (await promoResponse.json()) as ResponseData;
    return res.status(promoResponse.status).json(responseBody);
  } catch (error) {
    captureException(error);
    return res.status(502).json({
      error: "Unable to load this promo code. Please try again.",
    });
  }
}
