export const LOCAL_GATEWAY_URL = "http://127.0.0.1:8080";

export type ModelListSummary = {
  object: string;
  count: number;
  firstModel: string | null;
};

export type ModelList = {
  object: string;
  data: Array<{ id: string }>;
};

export type ListModels = () => Promise<ModelListSummary>;

export function summarizeModels(models: ModelList): ModelListSummary {
  return {
    object: models.object,
    count: models.data.length,
    firstModel: models.data[0]?.id ?? null,
  };
}
