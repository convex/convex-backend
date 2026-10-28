import { listModels as listModelsWithFetch } from "./integrations/fetch";
import { defineListModelsAction } from "./integrations/listModelsAction";

export const listModels = defineListModelsAction(listModelsWithFetch);
