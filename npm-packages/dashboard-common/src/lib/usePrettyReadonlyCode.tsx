import { convexToJson, Value } from "convex/values";
import { useCallback, useEffect, useRef, useState } from "react";
import sizeof from "object-sizeof";
import {
  ReadonlyCode,
  type ReadonlyCodeProps,
} from "@common/elements/ReadonlyCode";

const MAX_CODE_SIZE_KB = 10;

// A hook for prettifying code in a performant way, using a web worker.
// Each instance of usePrettyReadonlyCode will hold onto and maintain its own web worker.
export function usePrettyReadonlyCode(
  value: Value,
  path: string,
  props?: Partial<ReadonlyCodeProps>,
) {
  const [prettyCode, setPrettyCode] = useState<string>();
  const [isTooBig, setIsTooBig] = useState<boolean>(false);
  const [isFormattingCode, setIsFormattingCode] = useState(false);
  const workerRef = useRef<Worker | undefined>(undefined);

  const workerCallback = useCallback((message: MessageEvent<string>) => {
    // Received formatted code.
    setPrettyCode(message.data);

    // No longer formatting.
    setIsFormattingCode(false);
  }, []);

  useEffect(() => {
    // Create a worker to stringify Values asynchronously as prettier-formatting large blocks of code
    // is expensive.
    workerRef.current = new Worker(
      new URL("../workers/prettierWorker", import.meta.url),
    );

    // Receive messages from the worker.
    workerRef.current?.addEventListener("message", workerCallback);

    // Close the worker when the component is unmounted.
    return () => {
      workerRef.current?.removeEventListener("message", workerCallback);
      workerRef.current?.terminate();
    };
  }, [workerCallback]);

  useEffect(() => {
    setIsTooBig(false);

    if (value !== undefined) {
      // If the value is too big, don't format it.
      if (sizeof(value) / 1000 > MAX_CODE_SIZE_KB) {
        setIsTooBig(true);
      }

      // Begin formatting code.
      setIsFormattingCode(true);
      // Tell the worker we want to format this value.
      workerRef.current?.postMessage(convexToJson(value));
    }
  }, [value, workerCallback]);

  return {
    loading: isFormattingCode,
    // Don't render code that's too large in monaco.
    component: isTooBig ? (
      <pre className="scrollbar max-w-full overflow-x-auto text-xs">
        <code>{prettyCode}</code>
      </pre>
    ) : (
      <ReadonlyCode path={path} code={prettyCode || ""} {...props} />
    ),
    stringValue: prettyCode,
  };
}
