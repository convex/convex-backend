import { describe, expect, test } from "vitest";
import { createHash } from "crypto";
import {
  carrierSource,
  checkNativeSize,
  elfGlibcMax,
  findNativeTarget,
  hostNativeTarget,
  nativeArtifactPath,
  nativeCargoCommand,
  nativeFunctionMap,
  nativeModuleConfigs,
  parseNativeManifest,
  readManifestSymbols,
  refSource,
  selectNativeBuild,
  targetRustflagsVar,
} from "./native.js";
import type { RustManifestEntry } from "../rust.js";

const enc = new TextEncoder();
const dec = new TextDecoder();

function entry(modulePath: string, name: string): RustManifestEntry {
  return {
    abiVersion: 1,
    modulePath,
    name,
    visibility: "public",
    args: null,
    returns: null,
  };
}

function cat(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

// --- synthetic ELF64 ------------------------------------------------------

const VADDR_SKEW = 0x1000;

function buildElf(
  symbols: { name: string; data: Uint8Array }[],
  glibc: string[] = [],
): Uint8Array {
  // string tables
  let dynstr = "\0";
  const nameOffs: number[] = [];
  for (const s of symbols) {
    nameOffs.push(dynstr.length);
    dynstr += s.name + "\0";
  }
  const verStrOffs: number[] = [];
  for (const g of glibc) {
    verStrOffs.push(dynstr.length);
    dynstr += g + "\0";
  }
  const dynstrB = enc.encode(dynstr);
  // data blob
  const dataOffs: number[] = [];
  let dataLen = 0;
  for (const s of symbols) {
    dataOffs.push(dataLen);
    dataLen += s.data.length;
  }
  const dataB = cat(...symbols.map((s) => s.data), new Uint8Array(0));
  const ehdr = 64;
  const phdr = 56;
  const dataStart = ehdr + phdr;
  const strStart = dataStart + dataB.length;
  const symStart = Math.ceil((strStart + dynstrB.length) / 8) * 8;
  const symB = new Uint8Array(24 * (symbols.length + 2));
  const sv = new DataView(symB.buffer);
  // symbol 0 null; symbol 1 undefined with a convex_mf name (must be skipped)
  sv.setUint32(24, nameOffs[0] ?? 0, true);
  symbols.forEach((s, i) => {
    const o = 24 * (i + 2);
    sv.setUint32(o, nameOffs[i], true);
    sv.setUint16(o + 6, 5, true);
    sv.setBigUint64(o + 8, BigInt(VADDR_SKEW + dataStart + dataOffs[i]), true);
    sv.setBigUint64(o + 16, BigInt(s.data.length), true);
  });
  // verneed: one Verneed with N Vernaux
  const verStart = symStart + symB.length;
  const verB = new Uint8Array(16 + 16 * glibc.length);
  const vv = new DataView(verB.buffer);
  vv.setUint16(0, 1, true);
  vv.setUint16(2, glibc.length, true);
  vv.setUint32(8, 16, true);
  glibc.forEach((_, i) => {
    const o = 16 + 16 * i;
    vv.setUint32(o + 8, verStrOffs[i], true);
    vv.setUint32(o + 12, i === glibc.length - 1 ? 0 : 16, true);
  });
  const shStart = Math.ceil((verStart + verB.length) / 8) * 8;
  const total = shStart + 64 * 4;
  const out = new Uint8Array(total);
  const v = new DataView(out.buffer);
  out.set([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1], 0);
  v.setUint16(16, 3, true);
  v.setBigUint64(32, BigInt(ehdr), true);
  v.setBigUint64(40, BigInt(shStart), true);
  v.setUint16(52, 64, true);
  v.setUint16(54, phdr, true);
  v.setUint16(56, 1, true);
  v.setUint16(58, 64, true);
  v.setUint16(60, 4, true);
  // PT_LOAD covering the whole file, skewed vaddr
  v.setUint32(ehdr, 1, true);
  v.setBigUint64(ehdr + 8, 0n, true);
  v.setBigUint64(ehdr + 16, BigInt(VADDR_SKEW), true);
  v.setBigUint64(ehdr + 32, BigInt(total), true);
  out.set(dataB, dataStart);
  out.set(dynstrB, strStart);
  out.set(symB, symStart);
  out.set(verB, verStart);
  // sections: 0 null, 1 dynstr, 2 dynsym (link 1), 3 verneed (link 1)
  const sh = (i: number, type: number, off: number, size: number, link = 0) => {
    const o = shStart + 64 * i;
    v.setUint32(o + 4, type, true);
    v.setBigUint64(o + 24, BigInt(off), true);
    v.setBigUint64(o + 32, BigInt(size), true);
    v.setUint32(o + 40, link, true);
  };
  sh(1, 3, strStart, dynstrB.length);
  sh(2, 11, symStart, symB.length, 1);
  sh(3, 0x6ffffffe, verStart, verB.length, 1);
  return out;
}

// --- synthetic Mach-O 64 --------------------------------------------------

function buildMachO(symbols: { name: string; data: Uint8Array }[]): Uint8Array {
  const SEG_ADDR = 0x100000000n;
  let strtab = "\0";
  const nameOffs: number[] = [];
  for (const s of symbols) {
    nameOffs.push(strtab.length);
    strtab += "_" + s.name + "\0";
  }
  // an unrelated symbol after the data, to bound the last manifest symbol
  const otherOff = strtab.length;
  strtab += "_other\0";
  const strB = enc.encode(strtab);
  const dataB = cat(...symbols.map((s) => s.data));
  const gap = 4; // padding between the manifest data and `_other`
  const sectSize = dataB.length + gap + 8;
  const hdr = 32;
  const segCmd = 72 + 80;
  const symCmd = 24;
  const dataOff = hdr + segCmd + symCmd;
  const symOff = Math.ceil((dataOff + sectSize) / 8) * 8;
  const nsyms = symbols.length + 1;
  const strOff = symOff + 16 * nsyms;
  const out = new Uint8Array(strOff + strB.length);
  const v = new DataView(out.buffer);
  v.setUint32(0, 0xfeedfacf, true);
  v.setUint32(16, 2, true);
  v.setUint32(20, segCmd + symCmd, true);
  // LC_SEGMENT_64 with one section
  v.setUint32(hdr, 0x19, true);
  v.setUint32(hdr + 4, segCmd, true);
  v.setUint32(hdr + 64, 1, true);
  const so = hdr + 72;
  v.setBigUint64(so + 32, SEG_ADDR + 0x4000n, true);
  v.setBigUint64(so + 40, BigInt(sectSize), true);
  v.setUint32(so + 48, dataOff, true);
  // LC_SYMTAB
  const sc = hdr + segCmd;
  v.setUint32(sc, 2, true);
  v.setUint32(sc + 4, symCmd, true);
  v.setUint32(sc + 8, symOff, true);
  v.setUint32(sc + 12, nsyms, true);
  v.setUint32(sc + 16, strOff, true);
  v.setUint32(sc + 20, strB.length, true);
  out.set(dataB, dataOff);
  out.fill(0, dataOff + dataB.length, dataOff + dataB.length + gap);
  let off = 0;
  symbols.forEach((s, i) => {
    const o = symOff + 16 * i;
    v.setUint32(o, nameOffs[i], true);
    out[o + 4] = 0x0f; // N_SECT | N_EXT
    out[o + 5] = 1;
    v.setBigUint64(o + 8, SEG_ADDR + 0x4000n + BigInt(off), true);
    off += s.data.length;
  });
  const oo = symOff + 16 * symbols.length;
  v.setUint32(oo, otherOff, true);
  out[oo + 4] = 0x0f;
  out[oo + 5] = 1;
  v.setBigUint64(oo + 8, SEG_ADDR + 0x4000n + BigInt(dataB.length + gap), true);
  out.set(strB, strOff);
  return out;
}

const mfA = JSON.stringify(entry("bench_rust::cpu", "cpu_001"));
const mfB = JSON.stringify(entry("bench_rust::a::b", "other"));
const syms = [
  { name: "convex_mf:bench_rust::cpu:cpu_001", data: enc.encode(mfA) },
  { name: "convex_mf:bench_rust::a::b:other", data: enc.encode(mfB) },
];

describe("target table and selection", () => {
  test("table", () => {
    expect(findNativeTarget("aarch64-unknown-linux-gnu")).toEqual({
      triple: "aarch64-unknown-linux-gnu",
      os: "linux",
      arch: "aarch64",
      env: "gnu",
    });
    expect(findNativeTarget("x86_64-unknown-linux-musl")).toBeNull();
    expect(hostNativeTarget("darwin", "arm64")?.triple).toBe(
      "aarch64-apple-darwin",
    );
    expect(hostNativeTarget("linux", "x64")?.triple).toBe(
      "x86_64-unknown-linux-gnu",
    );
    expect(hostNativeTarget("win32", "x64")).toBeNull();
  });

  test("host equals backend uses plain cargo", () => {
    const host = hostNativeTarget("darwin", "arm64");
    expect(selectNativeBuild(host, { triple: "aarch64-apple-darwin" })).toEqual(
      {
        kind: "host",
        triple: "aarch64-apple-darwin",
      },
    );
  });

  test("mac host, linux backend uses zigbuild with glibc suffix", () => {
    const host = hostNativeTarget("darwin", "arm64");
    expect(
      selectNativeBuild(host, {
        triple: "aarch64-unknown-linux-gnu",
        glibc: "2.39",
      }),
    ).toEqual({
      kind: "zigbuild",
      triple: "aarch64-unknown-linux-gnu",
      glibc: "2.39",
      cargoTarget: "aarch64-unknown-linux-gnu.2.39",
    });
  });

  test("unsupported cases", () => {
    const linux = hostNativeTarget("linux", "arm64");
    expect(
      selectNativeBuild(linux, { triple: "aarch64-unknown-linux-musl" }).kind,
    ).toBe("unsupported");
    expect(
      selectNativeBuild(linux, { triple: "aarch64-apple-darwin" }).kind,
    ).toBe("unsupported");
    expect(
      selectNativeBuild(hostNativeTarget("darwin", "arm64"), {
        triple: "x86_64-unknown-linux-gnu",
      }).kind,
    ).toBe("unsupported");
  });
});

describe("cargo command", () => {
  test("release host build", () => {
    const cmd = nativeCargoCommand({
      manifestPath: "/p/rust/Cargo.toml",
      plan: { kind: "host", triple: "aarch64-apple-darwin" },
      mode: "release",
    });
    expect(cmd.command).toBe("cargo");
    expect(cmd.args).toEqual([
      "build",
      "--lib",
      "--manifest-path",
      "/p/rust/Cargo.toml",
      "--target",
      "aarch64-apple-darwin",
      "--release",
      "--config",
      'profile.release.panic="unwind"',
      "--config",
      'profile.release.strip="debuginfo"',
    ]);
    expect(cmd.env).toEqual({
      CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS:
        "--cfg convex_native --cfg aes_armv8 --cfg polyval_armv8",
    });
  });

  test("dev zigbuild", () => {
    const cmd = nativeCargoCommand({
      manifestPath: "m",
      plan: {
        kind: "zigbuild",
        triple: "aarch64-unknown-linux-gnu",
        glibc: "2.39",
        cargoTarget: "aarch64-unknown-linux-gnu.2.39",
      },
      mode: "dev",
      existingRustflags: "-C target-cpu=native",
    });
    expect(cmd.args.slice(0, 6)).toEqual([
      "zigbuild",
      "--lib",
      "--manifest-path",
      "m",
      "--target",
      "aarch64-unknown-linux-gnu.2.39",
    ]);
    expect(cmd.args).toContain("convex-dev");
    expect(cmd.args).toContain('profile.convex-dev.panic="unwind"');
    expect(cmd.env.CARGO_PROFILE_CONVEX_DEV_OPT_LEVEL).toBe("1");
    expect(cmd.env.CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS).toBe(
      "-C target-cpu=native --cfg convex_native --cfg aes_armv8 --cfg polyval_armv8",
    );
    expect(targetRustflagsVar("x86_64-unknown-linux-gnu")).toBe(
      "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
    );
  });

  test("x86_64 gets no armv8 cfgs", () => {
    const cmd = nativeCargoCommand({
      manifestPath: "m",
      plan: { kind: "host", triple: "x86_64-unknown-linux-gnu" },
      mode: "release",
      existingRustflags: "-C opt-level=3",
    });
    expect(cmd.env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS).toBe(
      "-C opt-level=3 --cfg convex_native",
    );
  });

  test("artifact path", () => {
    expect(
      nativeArtifactPath("/t", "aarch64-apple-darwin", "release", "bench-rust"),
    ).toBe("/t/aarch64-apple-darwin/release/libbench_rust.dylib");
    expect(
      nativeArtifactPath("/t", "x86_64-unknown-linux-gnu", "dev", "app"),
    ).toBe("/t/x86_64-unknown-linux-gnu/convex-dev/libapp.so");
    expect(() =>
      nativeArtifactPath("/t", "wasm32-unknown-unknown", "dev", "a"),
    ).toThrow();
  });
});

describe("library reader", () => {
  test("ELF64", () => {
    const elf = buildElf(syms, ["GLIBC_2.17", "GLIBC_2.34", "GLIBC_2.5"]);
    const found = readManifestSymbols(elf);
    expect([...found.keys()].sort()).toEqual([
      "bench_rust::a::b:other",
      "bench_rust::cpu:cpu_001",
    ]);
    expect(dec.decode(found.get("bench_rust::cpu:cpu_001"))).toBe(mfA);
    expect(parseNativeManifest(elf).map((e) => e.name)).toEqual([
      "other",
      "cpu_001",
    ]);
    expect(elfGlibcMax(elf)).toBe("2.34");
    expect(elfGlibcMax(buildElf(syms))).toBeNull();
  });

  test("Mach-O 64", () => {
    const macho = buildMachO(syms);
    const found = readManifestSymbols(macho);
    expect(dec.decode(found.get("bench_rust::cpu:cpu_001"))).toBe(mfA);
    expect(dec.decode(found.get("bench_rust::a::b:other"))).toBe(mfB);
    expect(parseNativeManifest(macho)).toHaveLength(2);
  });

  test("Mach-O 64: an entry that runs into unrelated data stops at its newline", () => {
    const macho = buildMachO([
      { name: syms[0].name, data: enc.encode(mfA + "\n") },
      {
        name: syms[1].name,
        data: enc.encode(mfB + "\ncalled `Result::unwrap()` on an `Err` value"),
      },
    ]);
    const found = readManifestSymbols(macho);
    expect(dec.decode(found.get("bench_rust::cpu:cpu_001"))).toBe(mfA);
    expect(dec.decode(found.get("bench_rust::a::b:other"))).toBe(mfB);
    expect(
      parseNativeManifest(macho)
        .map((e) => e.name)
        .sort(),
    ).toEqual(["cpu_001", "other"]);
  });

  test("rejects other formats and corrupt input", () => {
    expect(() => readManifestSymbols(new Uint8Array(16))).toThrow(/neither/);
    expect(() =>
      readManifestSymbols(new Uint8Array([0x7f, 0x45, 0x4c, 0x46])),
    ).toThrow();
    const elf = buildElf(syms);
    expect(() => readManifestSymbols(elf.subarray(0, 100))).toThrow(/Corrupt/);
    const dup = buildElf([syms[0], { ...syms[0], name: "convex_mf:x::y:z" }]);
    expect(() => parseNativeManifest(dup)).toThrow(/duplicate/);
  });
});

describe("size thresholds", () => {
  const MB = 1024 * 1024;
  test("levels", () => {
    expect(checkNativeSize(100 * MB).level).toBe("ok");
    expect(checkNativeSize(100 * MB + 1).level).toBe("warn");
    expect(checkNativeSize(180 * MB).level).toBe("warn");
    expect(checkNativeSize(180 * MB + 1).level).toBe("error");
  });
});

describe("carrier and ref", () => {
  const binary = new Uint8Array([1, 2, 3, 250, 251]);
  const sha = createHash("sha256").update(binary).digest("hex");
  const entries = [
    entry("bench_rust::cpu", "cpu_001"),
    entry("bench_rust::a::b", "other"),
    entry("bench_rust::cpu", "cpu_000"),
  ];
  const info = {
    triple: "aarch64-unknown-linux-gnu",
    glibcMax: "2.34",
    sdk: "0.1.0",
  };

  test("function map", () => {
    expect(nativeFunctionMap(entries)).toEqual({
      "a/b:other": "convex_run:bench_rust::a::b:other",
      "cpu:cpu_000": "convex_run:bench_rust::cpu:cpu_000",
      "cpu:cpu_001": "convex_run:bench_rust::cpu:cpu_001",
    });
  });

  test("carrier exact bytes", () => {
    expect(carrierSource(binary, entries, info)).toBe(
      "//convex-rust-native v1\n" +
        `{"format":1,"nativeAbi":1,"abi":1,"triple":"aarch64-unknown-linux-gnu","glibcMax":"2.34","sha256":"${sha}","size":5,` +
        `"functions":{"a/b:other":"convex_run:bench_rust::a::b:other","cpu:cpu_000":"convex_run:bench_rust::cpu:cpu_000","cpu:cpu_001":"convex_run:bench_rust::cpu:cpu_001"},"sdk":"0.1.0"}\n` +
        "AQID+vs=",
    );
    expect(
      carrierSource(binary, entries, { ...info, glibcMax: null }),
    ).toContain('"glibcMax":null,');
  });

  test("ref exact bytes", () => {
    expect(refSource("ab12", "rust/a/b.js")).toBe(
      '//convex-rust-native-ref v1\n{"sha256":"ab12","carrier":"rust/a/b.js"}',
    );
  });

  test("module configs", () => {
    const mods = nativeModuleConfigs(binary, entries, info);
    expect(mods.map((m) => m.path)).toEqual(["rust/a/b.js", "rust/cpu.js"]);
    expect(mods.every((m) => m.environment === "rust")).toBe(true);
    expect(mods[0].source).toBe(carrierSource(binary, entries, info));
    expect(mods[1].source).toBe(
      `//convex-rust-native-ref v1\n{"sha256":"${sha}","carrier":"rust/a/b.js"}`,
    );
    expect(nativeModuleConfigs(binary, [], info)).toEqual([]);
  });
});
