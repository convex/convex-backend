import tls from "node:tls";
import { createHash, X509Certificate } from "node:crypto";

// Userspace backport of the eMudhra fixes in Node.js 26.12.1, 24.22.1 and
// 22.23.4 (https://nodejs.org/en/blog/vulnerability/october-2026-security-releases)
// for Lambda runtimes that still ship an older Node.js. Remove once every
// supported runtime includes nodejs/node#66871 and #66877.

// SHA-256 of the SubjectPublicKeyInfo of the eight eMudhra/emSign
// intermediates whose private keys were reported compromised in October 2026.
// Byte-identical to Go's `distrustedSPKIHashes` (go1.27.3) and nodejs/node#66871.
export const DISTRUSTED_SPKI_SHA256 = new Set([
  "ebed6475c2c2d80fcb199f3fe8dd6e6b3ce60b77eb15d4e42c3dec0c6f5b5105",
  "49dd353dc2b919a3770645117d0e60c16115e7d1bc259eefe734b00ea434fe5f",
  "e0254c7fcdc265e2f0f8a428dabb75ab4ae039b4872d62f5ee57f0877d5a8a41",
  "5c0660a2b164910d8ff589c18e14552e9e0ec237dcefe2cb736d41a4811f81a0",
  "43bffa19d6ef651451f6841e019931e96f6f2eaeff28ab4b6f1ce4576de2717b",
  "63ecefee431d49523bc1136c4e72d0206a477086caf606cd5cf025686c109cf8",
  "ec4ffa1ad0ac88081314b7aaffef6a91f97cd57291bb5ba686c5415f5eb4f6b6",
  "92c063d67e8f14db71d288e11266ffdaf69cbf115f7bab14f3bc2f68d25e0e30",
]);

// SHA-256 fingerprints of the roots NSS 3.131.1 removed (nodejs/node#66877):
// emSign ECC Root CA - G3, emSign Root CA - C1, emSign ECC Root CA - C3.
const DISTRUSTED_ROOT_FINGERPRINTS = new Set([
  "86:A1:EC:BA:08:9C:4A:8D:3B:BE:27:34:C6:12:BA:34:1D:81:3E:04:3C:F9:E8:A8:62:CD:5C:57:A3:6B:BE:6B",
  "12:56:09:AA:30:1D:A0:A2:49:B9:7A:82:39:CB:6A:34:21:6F:44:DC:AC:9F:39:54:B1:42:92:F2:E8:C8:60:8F",
  "BC:4D:80:9B:15:18:9D:78:DB:3E:1D:8C:F4:F9:72:6A:79:5D:A1:64:3C:A5:F1:35:8E:1D:DB:0E:DC:0D:7E:B3",
]);

function spkiSha256(certDer: Buffer): string {
  const spki = new X509Certificate(certDer).publicKey.export({
    type: "spki",
    format: "der",
  });
  return createHash("sha256").update(spki).digest("hex");
}

export function findDistrustedCertificate(
  leaf: tls.DetailedPeerCertificate,
  distrusted: ReadonlySet<string> = DISTRUSTED_SPKI_SHA256,
): tls.PeerCertificate | undefined {
  // The issuer chain ends at a self-signed certificate whose
  // `issuerCertificate` points back to itself.
  const seen = new Set<tls.PeerCertificate>();
  let cert: tls.DetailedPeerCertificate | undefined = leaf;
  while (cert && cert.raw && !seen.has(cert)) {
    seen.add(cert);
    if (distrusted.has(spkiSha256(cert.raw))) {
      return cert;
    }
    cert = cert.issuerCertificate;
  }
  return undefined;
}

let installed = false;

// Set in the Lambda environment by the deploy worker in conductor.
export function distrustEmudhraCertificatesIfFlagged(
  env: NodeJS.ProcessEnv,
): boolean {
  if (env.CONVEX_DISTRUST_EMUDHRA_CERTIFICATES !== "true") {
    return false;
  }
  distrustEmudhraCertificates();
  return true;
}

export function distrustEmudhraCertificates() {
  if (installed) {
    return;
  }
  installed = true;

  // `_tls_wrap` reads `tls.checkServerIdentity` from the module exports on
  // every `tls.connect` without an explicit `checkServerIdentity`, and passes
  // it the full peer chain, so this covers `https`, `fetch` (undici) and
  // user-bundled clients alike. It runs only after OpenSSL verification
  // succeeded and is skipped on resumed sessions, matching nodejs/node#66871.
  const originalCheckServerIdentity = tls.checkServerIdentity;
  tls.checkServerIdentity = function checkServerIdentity(hostname, cert) {
    // An exception here escapes `onConnectSecure` and kills the process.
    // Returning it rejects only this connection.
    let distrusted: tls.PeerCertificate | undefined;
    try {
      distrusted = findDistrustedCertificate(
        cert as tls.DetailedPeerCertificate,
      );
    } catch (e) {
      return e instanceof Error ? e : new Error(String(e));
    }
    if (distrusted) {
      const err = new Error(
        `certificate chain for ${hostname} contains a distrusted public key (${distrusted.subject?.CN ?? "unknown subject"})`,
      ) as Error & { code: string };
      err.code = "CERT_REVOKED";
      return err;
    }
    return originalCheckServerIdentity(hostname, cert);
  };

  // `getCACertificates`/`setDefaultCACertificates` are missing on older
  // runtimes (e.g. Node.js 20), which rely on the SPKI check alone; that
  // already rejects every chain through the compromised intermediates.
  const { getCACertificates, setDefaultCACertificates } = tls as any;
  if (
    typeof getCACertificates === "function" &&
    typeof setDefaultCACertificates === "function"
  ) {
    const defaults: string[] = getCACertificates("default");
    const kept = defaults.filter(
      (pem) =>
        !DISTRUSTED_ROOT_FINGERPRINTS.has(
          new X509Certificate(pem).fingerprint256,
        ),
    );
    if (kept.length !== defaults.length) {
      setDefaultCACertificates(kept);
    }
  }
}
