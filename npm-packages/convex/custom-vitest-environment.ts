import type { Environment } from "vitest";
import { builtinEnvironments } from "vitest/runtime";

import ws from "ws";
const nodeWebSocket = ws as unknown as typeof WebSocket;

const happy = builtinEnvironments["happy-dom"];

export default <Environment>{
  name: "happy-dom-plus-ws",
  viteEnvironment: "client",
  // optional - only if you support "experimental-vm" pool
  async setupVM(options) {
    const { getVmContext: happyGetVmContext, teardown: happyTeardown } =
      await happy.setupVM!(options);
    return {
      getVmContext() {
        const context = happyGetVmContext();
        return context;
      },
      teardown() {
        return happyTeardown();
        // called after all tests with this env have been run
      },
    };
  },
  async setup(global, options) {
    const { teardown: happyTeardown } = await happy.setup(global, options);
    //populateGlobal(global, original, {});
    // Add the websocket here!
    global.myNewGlobalVariable = 8;
    global.WebSocket = nodeWebSocket;

    // custom setup
    return {
      teardown(global) {
        const ret = happyTeardown(global);
        delete global.myNewGlobalVariable;
        return ret;
        // called after all tests with this env have been run
      },
    };
  },
};
