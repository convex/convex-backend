import { Auth, MutationAuth } from "../authentication.js";
import { version } from "../../index.js";
import { performAsyncSyscall } from "./syscall.js";

export function setupAuth(requestId: string): Auth {
  return {
    getUserIdentity: async () => {
      return await performAsyncSyscall("1.0/getUserIdentity", {
        requestId,
      });
    },
  };
}

export function setupMutationAuth(requestId: string): MutationAuth {
  return {
    ...setupAuth(requestId),
    signJwt: async (options) => {
      return await performAsyncSyscall("1.0/signJwt", {
        requestId,
        version,
        ...options,
      });
    },
  };
}
