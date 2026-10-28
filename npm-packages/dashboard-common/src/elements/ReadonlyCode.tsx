import type { editor } from "monaco-editor";
import Editor, {
  DiffEditor,
  DiffEditorProps,
  EditorProps,
} from "@monaco-editor/react";
import { RefObject, useEffect, useRef, useState } from "react";
import { editorOptions } from "@common/elements/ObjectEditor/ObjectEditor";
import { useCurrentTheme } from "../lib/useCurrentTheme";

// The editor will have a height of 100% and will scroll.
type ParentHeight = {
  type: "parent";
};

// The editor will always have it's height set to the height of the content.
// You can consider setting this to `false` if you are experiencing issues with
// the editor growing infinitely in height in your layout.
type ContentHeight = {
  type: "content";
  // The editor's height will not exceed this value if it's defined. If the
  // content's length is greater than this height, then the editor's height will
  // be capped at this value and will scroll. If the content's height is less
  // than this value, then the editor will shrink to match the content height.
  maxHeightRem?: number;
};

export type HighlightLines = {
  startLineNumber: number;
  endLineNumber: number;
};

const maxHeightPixels = (heightObj: ContentHeight) => {
  const { maxHeightRem } = heightObj;
  if (!maxHeightRem) {
    return Number.MAX_SAFE_INTEGER;
  }
  const fontSize = parseFloat(
    getComputedStyle(document.documentElement).fontSize || "16",
  );
  return Math.round(maxHeightRem * fontSize);
};

function sharedEditorProps(
  height: ParentHeight | ContentHeight,
  prefersDark: boolean,
  disableLineNumbers: boolean,
): EditorProps & DiffEditorProps {
  return {
    height: "100%",
    theme: prefersDark ? "vs-dark" : "light",
    // Necessary to hide the cursor in the readonly editor. See globals.css
    className: "readonlyEditor",
    options: {
      ...editorOptions,
      readOnly: true,
      wordWrap: "on",
      domReadOnly: true,
      lineNumbers: disableLineNumbers ? "off" : "on",
      hover: { enabled: false },
      scrollbar: {
        horizontalScrollbarSize: 8,
        verticalScrollbarSize: 8,
        alwaysConsumeMouseWheel: false,
        useShadows: false,
        vertical:
          height.type === "content" && height.maxHeightRem === undefined
            ? "hidden"
            : "visible",
      },
      glyphMargin: !disableLineNumbers,
      lineDecorationsWidth: disableLineNumbers ? 0 : 10,
      lineNumbersMinChars: disableLineNumbers ? 0 : 5,
      folding: !disableLineNumbers,
    },
  };
}

function setupAutoHeight(
  editor: editor.ICodeEditor,
  ref: RefObject<HTMLDivElement | null>,
  maxHeight: number,
  variant: "editor" | "diff",
) {
  const updateHeight = (e: editor.IContentSizeChangedEvent) => {
    if (!e.contentHeightChanged || !ref.current) {
      return;
    }
    const contentHeight = Math.min(maxHeight, editor.getContentHeight());

    ref.current.style.height = `${contentHeight}px`;
    editor.layout({
      height: contentHeight,
      width:
        variant === "diff"
          ? ref.current.offsetWidth / 2
          : ref.current.offsetWidth,
    });
  };
  editor.onDidContentSizeChange(updateHeight);
}

export type ReadonlyCodeProps = {
  code: string;
  language?: string;
  highlightLines?: HighlightLines;
  path: string;
  height?: ParentHeight | ContentHeight;
  disableLineNumbers?: boolean;
};

export function ReadonlyCode({
  code,
  language = "json",
  highlightLines,
  path,
  height = { type: "parent" },
  disableLineNumbers = false,
}: ReadonlyCodeProps) {
  const [editor, setEditor] = useState<any>();
  // Track the decoration IDs we've applied so we can replace them when
  // `highlightLines` changes (or clear them when it goes away) — otherwise
  // stale highlights pile up while a panel stays open.
  const decorationIds = useRef<string[]>([]);
  useEffect(() => {
    if (!editor) {
      return;
    }

    // Paint the selected line, replacing any previously-painted lines.
    decorationIds.current = editor.deltaDecorations(
      decorationIds.current,
      highlightLines
        ? [
            {
              range: highlightLines,
              options: {
                isWholeLine: true,
                marginClassName: "monacoLineHighlight",
                inlineClassName: "monacoLineHighlight",
              },
            },
          ]
        : [],
    );

    if (highlightLines) {
      editor.revealLineNearTop(highlightLines.startLineNumber);
    }
  }, [editor, highlightLines, path]);

  const ref = useRef<HTMLDivElement>(null);

  // code.length * 18 is a hack from
  // https://github.com/microsoft/monaco-editor/issues/794#issuecomment-383523405
  // If it's wrong (probably due to font size changes), worst case there will
  // be a bit of a UI flash from our incorrect guess to the correct value that's
  // set in `updateHeight` based on the actual content size below.
  let initialHeight;
  if (height.type === "content") {
    const contentHeightGuessPixels = (code?.split("\n").length ?? 0) * 18;
    initialHeight = {
      height: Math.min(contentHeightGuessPixels, maxHeightPixels(height)),
    };
  } else {
    initialHeight = {
      height: "100%",
    };
  }

  const currentTheme = useCurrentTheme();
  const prefersDark = currentTheme === "dark";
  return (
    <div ref={ref} style={initialHeight} key={path}>
      <Editor
        value={code}
        path={path}
        language={language}
        onMount={(e) => {
          setEditor(e);
          if (height.type === "content") {
            setupAutoHeight(e, ref, maxHeightPixels(height), "editor");
          }
          e.revealLineNearTop(highlightLines?.startLineNumber ?? 1);
        }}
        {...sharedEditorProps(height, prefersDark, disableLineNumbers)}
      />
    </div>
  );
}

export function ReadonlyCodeDiff({
  originalCode,
  modifiedCode,
  language = "json",
  path,
  height = { type: "parent" },
}: {
  originalCode: string;
  modifiedCode: string;
  language?: string;
  path: string;
  height?: ParentHeight | ContentHeight;
}) {
  const ref = useRef<HTMLDivElement>(null);

  // Since there is no simple way to pre-compute the initial height of a diff,
  // we default to 200px and wait for the first onMount event handler
  // to set the actual height
  const initialHeight =
    height.type === "content" ? { height: "200px" } : { height: "100%" };

  const currentTheme = useCurrentTheme();
  const prefersDark = currentTheme === "dark";
  return (
    <div ref={ref} style={initialHeight}>
      <DiffEditor
        original={originalCode}
        modified={modifiedCode}
        path={path}
        language={language}
        onMount={(e) => {
          if (height.type === "content") {
            for (const editor of [
              e.getOriginalEditor(),
              e.getModifiedEditor(),
            ]) {
              setupAutoHeight(editor, ref, maxHeightPixels(height), "diff");
              setupAutoHeight(editor, ref, maxHeightPixels(height), "diff");
            }
          }
        }}
        {...sharedEditorProps(height, prefersDark, false)}
      />
    </div>
  );
}
