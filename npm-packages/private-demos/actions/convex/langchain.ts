import { OpenAI } from "@langchain/openai";
import { action, env } from "./_generated/server";

export default action(async (_, { prompt }: { prompt: string }) => {
  const model = new OpenAI({
    modelName: "gpt-4",
    temperature: 0.7,
    maxTokens: 1000,
    maxRetries: 5,
    openAIApiKey: env.OPENAI_API_KEY,
  });
  const res = await model.call(`Question: ${prompt} \nAnswer: `);
  console.log({ res });
  return res;
});
