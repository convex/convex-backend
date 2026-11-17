import { defineApp } from "convex/server";
import jobs from "../jobs/convex.config";

const app = defineApp();
app.use(jobs);

export default app;
