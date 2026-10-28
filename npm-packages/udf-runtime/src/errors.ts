import { performOp } from "./syscall";

interface FrameData {
  typeName: string | null;
  functionName: string | null;
  methodName: string | null;
  fileName: string | null;
  lineNumber: number | null;
  columnNumber: number | null;
  evalOrigin: string | null;
  isToplevel: boolean | null;
  isEval: boolean;
  isNative: boolean;
  isConstructor: boolean;
  isAsync: boolean;
  isPromiseAll: boolean;
  promiseIndex: number | null;
}

export function setupSourceMapping() {
  // This function is called on-demand when the `stack` property of an `Error` is accessed for the first time.
  // See https://v8.dev/docs/stack-trace-api for more details.
  Error.prepareStackTrace = (error, stackFrames) => {
    const frameData: FrameData[] = stackFrames.map((v8Frame) => {
      return {
        typeName: v8Frame.getTypeName(),
        functionName: v8Frame.getFunctionName(),
        methodName: v8Frame.getMethodName(),
        fileName: v8Frame.getFileName(),
        lineNumber: v8Frame.getLineNumber(),
        columnNumber: v8Frame.getColumnNumber(),
        evalOrigin: v8Frame.getEvalOrigin() ?? null,
        isToplevel: v8Frame.isToplevel(),
        isEval: v8Frame.isEval(),
        isNative: v8Frame.isNative(),
        isConstructor: v8Frame.isConstructor(),
        isAsync: (v8Frame as any).isAsync() as boolean,
        isPromiseAll: (v8Frame as any).isPromiseAll() as boolean,
        promiseIndex: (v8Frame as any).getPromiseIndex() as number | null,
      };
    });
    // We currently always go through JSON when going over the JS <-> Rust boundary. Eventually we can make this more efficient by accessing the V8 objects directly in Rust.
    const frameJSON = JSON.stringify(frameData);
    // Save the structured frame data on the exception so we can use it from Rust later.
    Object.defineProperties(error, {
      __frameData: { value: frameJSON, configurable: true },
    });
    const stack: string = performOp("error/stack", frameData);
    // JsError::to_string() has a trailing newline.
    // By removing the trailing newline, our stack trace format matches Chrome.
    return stack.trimEnd();
  };
}
