import { performAsyncOp, performOp } from "./syscall";
import { Blob, File } from "./09_file";
import { constructStreamId } from "./06_streams";

type FormDataEntryValue = string | File;

export class FormData {
  private _entries: [string, FormDataEntryValue][] = [];

  private addValue(name: string, value: FormDataEntryValue) {
    this._entries.push([name, value]);
  }

  append(name: string, value: Blob, filename?: string): void;
  append(name: string, value: string): void;
  append(name: string, value: string | Blob, filename?: string) {
    if (value instanceof File) {
      this.addValue(
        name,
        new File([value], filename ?? value.name, { type: value.type }),
      );
    } else if (value instanceof Blob) {
      this.addValue(
        name,
        new File([value], filename ?? "blob", { type: value.type }),
      );
    } else {
      this.addValue(name, `${value}`.toWellFormed());
    }
  }

  delete(name: string) {
    this._entries = this._entries.filter((entry) => entry[0] !== name);
  }

  *entries(): IterableIterator<[string, FormDataEntryValue]> {
    for (const [key, value] of this._entries) {
      yield [key, value];
    }
  }

  forEach(
    callbackfn: (
      value: FormDataEntryValue,
      key: string,
      parent: FormData,
    ) => void,
    thisArg?: unknown,
  ) {
    this._entries.forEach(([key, value]) => {
      callbackfn.call(thisArg, value, key, this);
    });
  }

  get(name: string): FormDataEntryValue | null {
    const entry = this._entries.find((entry) => entry[0] === name);
    if (!entry) {
      return null;
    }
    return entry[1];
  }

  getAll(name: string): FormDataEntryValue[] {
    return this._entries
      .filter((entry) => entry[0] === name)
      .map((entry) => entry[1]);
  }

  has(name: string): boolean {
    return !!this._entries.find((entry) => entry[0] === name);
  }

  keys(): IterableIterator<string> {
    return this._entries.map((entry) => entry[0]).values();
  }

  set(name: string, value: Blob, filename?: string): void;
  set(name: string, value: string): void;
  set(name: string, value: string | Blob, filename?: string) {
    let newValue;
    if (value instanceof Blob) {
      newValue = new File([value], filename ?? "");
    } else {
      newValue = `${value}`.toWellFormed();
    }
    // Insert in the same location as the first existing entry with the same
    // name, and delete the rest.
    let added = false;
    for (let i = 0; i < this._entries.length; i++) {
      if (this._entries[i][0] === name) {
        if (added) {
          this._entries.splice(i, 1);
          i--;
        } else {
          this._entries[i][1] = newValue;
          added = true;
        }
      }
    }
    if (!added) {
      this._entries.push([name, newValue]);
    }
  }

  values(): IterableIterator<FormDataEntryValue> {
    return this._entries.map((entry) => entry[1]).values();
  }

  [Symbol.iterator](): IterableIterator<[string, FormDataEntryValue]> {
    return this.entries();
  }
}

Object.defineProperty(FormData.prototype, Symbol.toStringTag, {
  value: "FormData",
  enumerable: false,
  writable: false,
  configurable: true,
});

export type MimeType = {
  essence: string;
  boundary: string | null;
};

export const parseFormData = async (
  body: Blob | null,
  contentType: string | null,
) => {
  if (contentType === null) {
    throw new TypeError("Missing content type");
  }
  const mimeType = performOp("headers/getMimeType", contentType);
  if (mimeType === null) {
    throw new TypeError("Invalid content type");
  }
  if (mimeType.essence === "multipart/form-data") {
    const boundary = mimeType.boundary;
    if (boundary === null) {
      throw new TypeError(
        "Missing boundary parameter in mime type of multipart formdata.",
      );
    }
    const entries = await performAsyncOp(
      "form/parseMultiPart",
      contentType,
      constructStreamId((body ?? new Blob()).stream()),
    );
    const formData = new FormData();
    for (const formEntry of entries) {
      if (formEntry.file !== null) {
        formData.append(
          formEntry.name,
          new Blob([formEntry.file.data], {
            // A part without a Content-Type header arrives as `null`, which
            // the Blob constructor would stringify to "null".
            type: formEntry.file.contentType ?? undefined,
          }),
          formEntry.file.fileName,
        );
      } else {
        formData.append(formEntry.name, formEntry.text);
      }
    }
    return formData;
  } else if (mimeType.essence === "application/x-www-form-urlencoded") {
    const entries = performOp(
      "url/getUrlSearchParamPairs",
      await (body ?? new Blob()).text(),
    );
    const formData = new FormData();
    for (const [k, v] of entries) {
      formData.append(k, v);
    }
    return formData;
  }
  throw new TypeError("Body cannot be decoded as form data");
};

const CRLF = "\r\n";
const LONELY_CR_OR_LF = /\r(?!\n)|(?<!\r)\n/g;
const fixLineEndings = (s: string) => {
  return s.replace(LONELY_CR_OR_LF, CRLF);
};

const escape = (s: string) => {
  return s.replace(/([\r\n"])/g, (c) => {
    switch (c) {
      case "\n":
        return "%0A";
      case "\r":
        return "%0D";
      case '"':
        return "%22";
    }
    return "";
  });
};

export const formDataToBlob = (formData: FormData) => {
  // Random string of digits.
  const boundary = `${Math.random()}${Math.random()}`
    .replace(".", "")
    .slice(-28);
  // https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#multipart-form-data

  const parts: FormDataEntryValue[] = [];
  const prefix = `--${boundary}\r\nContent-Disposition: form-data; name="`;
  for (const [name, value] of formData) {
    const escapedName = escape(fixLineEndings(name));
    if (typeof value === "string") {
      const escapedValue = fixLineEndings(value);
      parts.push(
        prefix + escapedName + '"' + CRLF + CRLF + escapedValue + CRLF,
      );
    } else {
      const escapedFilename = escape(value.name);
      const contentType = value.type || "application/octet-stream";
      parts.push(
        prefix +
          escapedName +
          '"; filename="' +
          escapedFilename +
          '"' +
          CRLF +
          "Content-Type: " +
          contentType +
          CRLF +
          CRLF,
      );
      parts.push(value);
      parts.push(CRLF);
    }
  }
  parts.push(`--${boundary}--`);
  return new Blob(parts, {
    type: "multipart/form-data; boundary=" + boundary,
  });
};

export const setupFormData = (global: any) => {
  global.FormData = FormData;
};
