import { defineApp } from "convex/server";
import myComponent from "./myComponent/convex.config";

const app = defineApp();
app.use(myComponent);
export default app;
