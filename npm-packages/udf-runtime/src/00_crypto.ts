// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
// https://github.com/denoland/deno/blob/main/ext/crypto/00_crypto.js

import {
  requiredArguments,
  throwNotImplementedMethodError,
  throwUncatchableDeveloperError,
} from "./helpers.js";
import { performOp } from "udf-syscall-ffi";

const INTEGER_TYPED_ARRAYS = [
  Int8Array,
  Uint8Array,
  Uint8ClampedArray,
  Int16Array,
  Uint16Array,
  Int32Array,
  Uint32Array,
  BigInt64Array,
  BigUint64Array,
];

class Crypto {
  constructor() {
    throwUncatchableDeveloperError("Illegal constructor: Crypto");
  }

  getRandomValues(typedArray: ArrayBufferView) {
    const prefix = "Failed to execute 'getRandomValues' on 'Crypto'";
    requiredArguments(arguments.length, 1, prefix);
    if (!INTEGER_TYPED_ARRAYS.some((ctor) => typedArray instanceof ctor)) {
      throw new DOMException(
        "The provided ArrayBufferView is not an integer array type",
        "TypeMismatchError",
      );
    }
    const randomValues = performOp(
      "crypto/getRandomValues",
      typedArray.byteLength,
    );
    // TODO: CX-4404
    if (typedArray instanceof Uint8Array) {
      typedArray.set(randomValues);
      return typedArray;
    }
    const ui8 = new Uint8Array(
      typedArray.buffer,
      typedArray.byteOffset,
      typedArray.byteLength,
    );
    ui8.set(randomValues);
    return typedArray;
  }

  randomUUID() {
    return performOp("crypto/randomUUID");
  }

  get subtle() {
    return subtleImpl;
  }

  inspect() {
    return "Crypto {}";
  }
}

Object.defineProperties(Crypto.prototype, {
  [Symbol.toStringTag]: {
    value: "Crypto",
    enumerable: false,
    writable: false,
    configurable: true,
  },
  subtle: { enumerable: true },
  getRandomValues: { enumerable: true, configurable: true, writable: true },
  randomUUID: { enumerable: true, configurable: true, writable: true },
});

class SubtleCrypto {
  constructor() {
    throwUncatchableDeveloperError("Illegal constructor: SubtleCrypto");
  }

  async digest(
    algorithm: AlgorithmIdentifier,
    data: BufferSource,
  ): Promise<ArrayBuffer> {
    const prefix = "Failed to execute 'digest' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 2, prefix);
    return performOp("crypto/subtle/digest", algorithm, data) as ArrayBuffer;
  }

  async encrypt(
    algorithm:
      | AlgorithmIdentifier
      | RsaOaepParams
      | AesCtrParams
      | AesCbcParams
      | AesGcmParams,
    key: CryptoKey,
    data: BufferSource,
  ): Promise<ArrayBuffer> {
    const prefix = "Failed to execute 'encrypt' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 3, prefix);
    return performOp(
      "crypto/subtle/encrypt",
      algorithm,
      key,
      data,
    ) as ArrayBuffer;
  }

  async decrypt(
    algorithm:
      | AlgorithmIdentifier
      | RsaOaepParams
      | AesCtrParams
      | AesCbcParams
      | AesGcmParams,
    key: CryptoKey,
    data: BufferSource,
  ): Promise<ArrayBuffer> {
    const prefix = "Failed to execute 'decrypt' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 3, prefix);
    return performOp(
      "crypto/subtle/decrypt",
      algorithm,
      key,
      data,
    ) as ArrayBuffer;
  }

  async sign(
    algorithm: AlgorithmIdentifier | RsaPssParams | EcdsaParams,
    key: CryptoKey,
    data: BufferSource,
  ): Promise<ArrayBuffer> {
    const prefix = "Failed to execute 'sign' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 3, prefix);
    return performOp("crypto/subtle/sign", algorithm, key, data) as ArrayBuffer;
  }

  async importKey(
    format: any,
    keyData: any,
    algorithm: any,
    extractable: any,
    keyUsages: any,
  ) {
    const prefix = "Failed to execute 'importKey' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 5, prefix);
    return performOp(
      "crypto/subtle/importKey",
      format,
      keyData,
      algorithm,
      // coerce to boolean per Web IDL
      !!extractable,
      keyUsages,
    );
  }

  async exportKey(
    format: "jwk" | "pkcs8" | "raw" | "spki",
    key: CryptoKey,
  ): Promise<ArrayBuffer | JsonWebKey> {
    const prefix = "Failed to execute 'exportKey' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 2, prefix);
    const result = performOp("crypto/subtle/exportKey", format, key) as
      | ArrayBuffer
      | JsonWebKey;
    return result;
  }

  async deriveBits(algorithm: any, baseKey: any, length: any) {
    const prefix = "Failed to execute 'deriveBits' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 3, prefix);
    return performOp(
      "crypto/subtle/deriveBits",
      algorithm,
      baseKey,
      length,
    ) as ArrayBuffer;
  }

  async deriveKey(
    algorithm: any,
    baseKey: any,
    derivedKeyType: any,
    extractable: any,
    keyUsages: any,
  ) {
    const prefix = "Failed to execute 'deriveKey' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 5, prefix);
    return performOp(
      "crypto/subtle/deriveKey",
      algorithm,
      baseKey,
      derivedKeyType,
      !!extractable,
      keyUsages,
    );
  }

  async verify(
    algorithm: AlgorithmIdentifier | RsaPssParams | EcdsaParams,
    key: CryptoKey,
    signature: BufferSource,
    data: BufferSource,
  ) {
    const prefix = "Failed to execute 'verify' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 4, prefix);
    return performOp(
      "crypto/subtle/verify",
      algorithm,
      key,
      signature,
      data,
    ) as boolean;
  }

  async wrapKey() {
    throwNotImplementedMethodError("wrapKey", "SubtleCrypto");
  }

  async unwrapKey() {
    throwNotImplementedMethodError("unwrapKey", "SubtleCrypto");
  }

  async generateKey(
    algorithm:
      | AlgorithmIdentifier
      | AesKeyGenParams
      | HmacKeyGenParams
      | RsaHashedKeyGenParams
      | EcKeyGenParams,
    extractable: boolean,
    keyUsages: ReadonlyArray<KeyUsage>,
  ): Promise<CryptoKeyPair | CryptoKey> {
    const prefix = "Failed to execute 'generateKey' on 'SubtleCrypto'";
    requiredArguments(arguments.length, 3, prefix);
    return performOp(
      "crypto/subtle/generateKey",
      algorithm,
      !!extractable,
      keyUsages,
    ) as CryptoKey | CryptoKeyPair;
  }

  inspect() {
    return "SubtleCrypto {}";
  }
}

Object.defineProperties(SubtleCrypto.prototype, {
  [Symbol.toStringTag]: {
    value: "SubtleCrypto",
    enumerable: false,
    writable: false,
    configurable: true,
  },
  encrypt: { enumerable: true, configurable: true, writable: true },
  decrypt: { enumerable: true, configurable: true, writable: true },
  sign: { enumerable: true, configurable: true, writable: true },
  verify: { enumerable: true, configurable: true, writable: true },
  digest: { enumerable: true, configurable: true, writable: true },
  generateKey: { enumerable: true, configurable: true, writable: true },
  deriveKey: { enumerable: true, configurable: true, writable: true },
  deriveBits: { enumerable: true, configurable: true, writable: true },
  importKey: { enumerable: true, configurable: true, writable: true },
  exportKey: { enumerable: true, configurable: true, writable: true },
  wrapKey: { enumerable: true, configurable: true, writable: true },
  unwrapKey: { enumerable: true, configurable: true, writable: true },
});

const subtleImpl: SubtleCrypto = Reflect.construct(
  function () {},
  [],
  SubtleCrypto,
);

export const setupCrypto = (global: any) => {
  Object.defineProperty(global, "Crypto", {
    value: Crypto,
    enumerable: false,
    configurable: true,
  });
  Object.defineProperty(global, "SubtleCrypto", {
    value: SubtleCrypto,
    enumerable: false,
    configurable: true,
  });
  const cryptoImpl = Reflect.construct(function () {}, [], Crypto);
  Object.defineProperty(global, "crypto", {
    get() {
      return cryptoImpl;
    },
    enumerable: true,
    configurable: true,
  });
};
