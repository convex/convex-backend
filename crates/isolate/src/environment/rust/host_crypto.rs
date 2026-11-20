//! Optional `crypto/*` sync ops (contracts/ABI.md "Optional sync ops", crypto
//! version 2): `crypto/caps`, `crypto/verify`, `crypto/sign`, `crypto/aead`,
//! `crypto/digest`, `crypto/hmac`, natively on aws-lc-rs. Wasm guests carry no
//! crypto code of their own; they call these. Served by the wasmtime core
//! itself, so every `HostBackend` gets it.
//!
//! Byte arguments are a base64 string or a `{"ptr", "len"}` reference into the
//! guest's linear memory, read in place (`crypto/aead` transforms `data` in
//! place), so large payloads are never copied through JSON.

use std::borrow::Cow;

use aws_lc_rs::{
    aead::{
        Aad,
        LessSafeKey,
        Nonce,
        UnboundKey,
        AES_128_GCM,
        AES_256_GCM,
    },
    digest,
    hmac,
    rand::SystemRandom,
    signature::{
        EcdsaKeyPair,
        Ed25519KeyPair,
        KeyPair,
        RsaKeyPair,
        RsaEncoding,
        UnparsedPublicKey,
        VerificationAlgorithm,
        ECDSA_P256_SHA256_FIXED,
        ECDSA_P256_SHA256_FIXED_SIGNING,
        ECDSA_P384_SHA384_FIXED,
        ECDSA_P384_SHA384_FIXED_SIGNING,
        ED25519,
        RSA_PKCS1_2048_8192_SHA256,
        RSA_PKCS1_2048_8192_SHA384,
        RSA_PKCS1_2048_8192_SHA512,
        RSA_PKCS1_SHA256,
        RSA_PKCS1_SHA384,
        RSA_PKCS1_SHA512,
        RSA_PSS_2048_8192_SHA256,
        RSA_PSS_2048_8192_SHA384,
        RSA_PSS_2048_8192_SHA512,
        RSA_PSS_SHA256,
        RSA_PSS_SHA384,
        RSA_PSS_SHA512,
    },
};
use serde_json::{
    json,
    Value as JsonValue,
};

const VERSION: u64 = 2;
const SIG_ALGS: [&str; 9] = [
    "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256", "ES384", "EdDSA",
];
const AEAD_ALGS: [&str; 2] = ["A128GCM", "A256GCM"];
const DIGEST_ALGS: [&str; 4] = ["SHA-1", "SHA-256", "SHA-384", "SHA-512"];
const TAG_LEN: usize = 16;
const NONCE_LEN: usize = 12;
/// RSA modulus policy, identical on every host (aws-lc-rs limits; the V8 host
/// enforces the same range).
const RSA_BITS: std::ops::RangeInclusive<usize> = 2048..=8192;

type Res<T> = Result<T, String>;

fn verify_algorithm(alg: &str) -> Option<&'static dyn VerificationAlgorithm> {
    Some(match alg {
        "RS256" => &RSA_PKCS1_2048_8192_SHA256,
        "RS384" => &RSA_PKCS1_2048_8192_SHA384,
        "RS512" => &RSA_PKCS1_2048_8192_SHA512,
        "PS256" => &RSA_PSS_2048_8192_SHA256,
        "PS384" => &RSA_PSS_2048_8192_SHA384,
        "PS512" => &RSA_PSS_2048_8192_SHA512,
        "ES256" => &ECDSA_P256_SHA256_FIXED,
        "ES384" => &ECDSA_P384_SHA384_FIXED,
        "EdDSA" => &ED25519,
        _ => return None,
    })
}

fn rsa_padding(alg: &str) -> Option<&'static dyn RsaEncoding> {
    Some(match alg {
        "RS256" => &RSA_PKCS1_SHA256,
        "RS384" => &RSA_PKCS1_SHA384,
        "RS512" => &RSA_PKCS1_SHA512,
        "PS256" => &RSA_PSS_SHA256,
        "PS384" => &RSA_PSS_SHA384,
        "PS512" => &RSA_PSS_SHA512,
        _ => return None,
    })
}

fn digest_algorithm(alg: &str) -> Option<&'static digest::Algorithm> {
    Some(match alg {
        "SHA-1" => &digest::SHA1_FOR_LEGACY_USE_ONLY,
        "SHA-256" => &digest::SHA256,
        "SHA-384" => &digest::SHA384,
        "SHA-512" => &digest::SHA512,
        _ => return None,
    })
}

fn hmac_algorithm(alg: &str) -> Option<hmac::Algorithm> {
    Some(match alg {
        "SHA-1" => hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
        "SHA-256" => hmac::HMAC_SHA256,
        "SHA-384" => hmac::HMAC_SHA384,
        "SHA-512" => hmac::HMAC_SHA512,
        _ => return None,
    })
}

/// Bit length of the modulus of a PKCS#1 `RSAPublicKey` DER, or `None` if
/// malformed.
fn rsa_modulus_bits(der: &[u8]) -> Option<usize> {
    fn len(b: &[u8]) -> Option<(usize, &[u8])> {
        let (&first, rest) = b.split_first()?;
        if first < 0x80 {
            return Some((first as usize, rest));
        }
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let v = rest[..n].iter().fold(0usize, |a, &x| (a << 8) | x as usize);
        Some((v, &rest[n..]))
    }
    let (&tag, rest) = der.split_first()?;
    if tag != 0x30 {
        return None;
    }
    let (_, rest) = len(rest)?;
    let (&tag, rest) = rest.split_first()?;
    if tag != 0x02 {
        return None;
    }
    let (n, rest) = len(rest)?;
    let m = rest.get(..n)?;
    let m = &m[m.iter().position(|&b| b != 0)?..];
    Some((m.len() - 1) * 8 + (8 - m[0].leading_zeros() as usize))
}

/// Largest `{ptr, len}` argument served inline. The op runs on the async
/// task and cannot be interrupted by the call's wall-clock limit, so a
/// guest-chosen length must not be able to pin a thread for long.
pub const MAX_INPUT_BYTES: usize = 32 << 20;

/// Total length of the `{ptr, len}` arguments of an op (cheap pre-check).
pub fn input_len(args: &JsonValue) -> usize {
    args.as_object().map_or(0, |o| {
        o.values()
            .filter_map(|v| v.get("len")?.as_u64())
            .fold(0usize, |a, l| a.saturating_add(l as usize))
    })
}

/// A resolved byte argument.
fn range_of(args: &JsonValue, field: &str, mem_len: usize) -> Option<Res<std::ops::Range<usize>>> {
    let v = args.get(field)?.as_object()?;
    let (ptr, len) = (v.get("ptr")?.as_u64()?, v.get("len")?.as_u64()?);
    let (ptr, len) = (ptr as usize, len as usize);
    Some(match ptr.checked_add(len) {
        _ if len > MAX_INPUT_BYTES => Err(format!("{field}: input too large (max {MAX_INPUT_BYTES} bytes)")),
        Some(end) if end <= mem_len => Ok(ptr..end),
        _ => Err(format!("{field}: out of bounds")),
    })
}

fn bytes<'a>(args: &JsonValue, field: &str, mem: &'a [u8]) -> Res<Cow<'a, [u8]>> {
    if let Some(r) = range_of(args, field, mem.len()) {
        return Ok(Cow::Borrowed(&mem[r?]));
    }
    let s = args
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing {field}"))?;
    base64::decode(s)
        .map(Cow::Owned)
        .map_err(|e| format!("bad {field}: {e}"))
}

fn alg_of<'a>(args: &'a JsonValue) -> Res<&'a str> {
    args.get("alg")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing alg".to_string())
}

fn check_rsa_public(alg: &str, key: &[u8]) -> Res<()> {
    if alg.starts_with("ES") || alg == "EdDSA" {
        return Ok(());
    }
    match rsa_modulus_bits(key) {
        Some(b) if RSA_BITS.contains(&b) => Ok(()),
        Some(b) => Err(format!("unsupported RSA key size {b}")),
        None => Err("bad RSA key".into()),
    }
}

fn verify(args: &JsonValue, mem: &[u8]) -> Res<JsonValue> {
    let alg = alg_of(args)?;
    let params = verify_algorithm(alg).ok_or_else(|| format!("unsupported alg {alg}"))?;
    let (key, msg, sig) = (bytes(args, "key", mem)?, bytes(args, "msg", mem)?, bytes(args, "sig", mem)?);
    check_rsa_public(alg, &key)?;
    if alg == "EdDSA" && key.len() != 32 {
        return Err("bad Ed25519 key".into());
    }
    Ok(json!(UnparsedPublicKey::new(params, &*key).verify(&msg, &sig).is_ok()))
}

fn sign(args: &JsonValue, mem: &[u8]) -> Res<JsonValue> {
    let alg = alg_of(args)?;
    let (key, msg) = (bytes(args, "key", mem)?, bytes(args, "msg", mem)?);
    let rng = SystemRandom::new();
    let sig: Vec<u8> = match alg {
        "ES256" | "ES384" => {
            let signing = if alg == "ES256" {
                &ECDSA_P256_SHA256_FIXED_SIGNING
            } else {
                &ECDSA_P384_SHA384_FIXED_SIGNING
            };
            let kp = EcdsaKeyPair::from_pkcs8(signing, &key).map_err(|e| format!("bad key: {e}"))?;
            kp.sign(&rng, &msg).map_err(|e| e.to_string())?.as_ref().to_vec()
        },
        "EdDSA" => {
            let kp =
                Ed25519KeyPair::from_pkcs8_maybe_unchecked(&key).map_err(|e| format!("bad key: {e}"))?;
            kp.sign(&msg).as_ref().to_vec()
        },
        _ => {
            let padding = rsa_padding(alg).ok_or_else(|| format!("unsupported alg {alg}"))?;
            let kp = RsaKeyPair::from_pkcs8(&key).map_err(|e| format!("bad key: {e}"))?;
            let bits = kp.public_modulus_len() * 8;
            if !RSA_BITS.contains(&bits) {
                return Err(format!("unsupported RSA key size {bits}"));
            }
            let mut sig = vec![0u8; kp.public_modulus_len()];
            kp.sign(padding, &rng, &msg, &mut sig).map_err(|e| e.to_string())?;
            sig
        },
    };
    Ok(json!(base64::encode(sig)))
}

fn pubkey(args: &JsonValue, mem: &[u8]) -> Res<JsonValue> {
    let alg = alg_of(args)?;
    let key = bytes(args, "key", mem)?;
    let public: Vec<u8> = match alg {
        "ES256" | "ES384" => {
            let signing = if alg == "ES256" {
                &ECDSA_P256_SHA256_FIXED_SIGNING
            } else {
                &ECDSA_P384_SHA384_FIXED_SIGNING
            };
            let kp = EcdsaKeyPair::from_pkcs8(signing, &key).map_err(|e| format!("bad key: {e}"))?;
            kp.public_key().as_ref().to_vec()
        },
        "EdDSA" => {
            let kp =
                Ed25519KeyPair::from_pkcs8_maybe_unchecked(&key).map_err(|e| format!("bad key: {e}"))?;
            kp.public_key().as_ref().to_vec()
        },
        _ => return Err(format!("unsupported alg {alg}")),
    };
    Ok(json!(base64::encode(public)))
}

fn aead(args: &JsonValue, mem: &mut [u8]) -> Res<JsonValue> {
    let alg = alg_of(args)?;
    let algorithm = match alg {
        "A128GCM" => &AES_128_GCM,
        "A256GCM" => &AES_256_GCM,
        _ => return Err(format!("unsupported alg {alg}")),
    };
    let dir = args.get("dir").and_then(|v| v.as_str()).unwrap_or_default();
    let key = bytes(args, "key", mem)?.into_owned();
    let nonce = bytes(args, "nonce", mem)?.into_owned();
    let aad = match args.get("aad") {
        Some(v) if !v.is_null() => bytes(args, "aad", mem)?.into_owned(),
        _ => Vec::new(),
    };
    let tag = if dir == "open" {
        bytes(args, "tag", mem)?.into_owned()
    } else {
        Vec::new()
    };
    if nonce.len() != NONCE_LEN {
        return Err(format!("nonce must be {NONCE_LEN} bytes"));
    }
    let key = UnboundKey::new(algorithm, &key)
        .map(LessSafeKey::new)
        .map_err(|_| format!("{alg} needs a {}-byte key", algorithm.key_len()))?;
    let nonce = Nonce::try_assume_unique_for_key(&nonce).map_err(|e| e.to_string())?;
    let range = range_of(args, "data", mem.len()).ok_or("data must be a memory reference")??;
    let data = &mut mem[range];
    match dir {
        "seal" => {
            let tag = key
                .seal_in_place_separate_tag(nonce, Aad::from(aad), data)
                .map_err(|e| e.to_string())?;
            Ok(json!({"tag": base64::encode(tag.as_ref())}))
        },
        "open" => {
            if tag.len() != TAG_LEN {
                return Err(format!("tag must be {TAG_LEN} bytes"));
            }
            let n = data.len();
            let mut buf = Vec::with_capacity(n + TAG_LEN);
            buf.extend_from_slice(data);
            buf.extend_from_slice(&tag);
            Ok(match key.open_in_place(nonce, Aad::from(aad), &mut buf) {
                Ok(pt) => {
                    data.copy_from_slice(pt);
                    json!(true)
                },
                Err(_) => json!(false),
            })
        },
        _ => Err(format!("bad dir {dir}")),
    }
}

fn digest_op(args: &JsonValue, mem: &[u8]) -> Res<JsonValue> {
    let alg = alg_of(args)?;
    let algorithm = digest_algorithm(alg).ok_or_else(|| format!("unsupported alg {alg}"))?;
    let data = bytes(args, "data", mem)?;
    Ok(json!(base64::encode(digest::digest(algorithm, &data))))
}

fn hmac_op(args: &JsonValue, mem: &[u8]) -> Res<JsonValue> {
    let alg = alg_of(args)?;
    let algorithm = hmac_algorithm(alg).ok_or_else(|| format!("unsupported alg {alg}"))?;
    let (key, data) = (bytes(args, "key", mem)?, bytes(args, "data", mem)?);
    Ok(json!(base64::encode(hmac::sign(&hmac::Key::new(algorithm, &key), &data))))
}

/// `Some(reply)` if `op` is one of ours, else `None` (goes to the backend).
/// `mem` is the guest's linear memory, for `{ptr, len}` byte arguments.
pub fn sync_op(op: &str, args: &JsonValue, mem: &mut [u8]) -> Option<JsonValue> {
    let r = match op {
        "crypto/caps" => {
            return Some(json!({"ok": {"version": VERSION, "ops": {
                "verify": SIG_ALGS, "sign": SIG_ALGS, "pubkey": ["ES256", "ES384", "EdDSA"], "aead": AEAD_ALGS,
                "digest": DIGEST_ALGS, "hmac": DIGEST_ALGS,
            }}}));
        },
        "crypto/verify" => verify(args, mem),
        "crypto/sign" => sign(args, mem),
        "crypto/pubkey" => pubkey(args, mem),
        "crypto/aead" => aead(args, mem),
        "crypto/digest" => digest_op(args, mem),
        "crypto/hmac" => hmac_op(args, mem),
        _ => return None,
    };
    Some(match r {
        Ok(v) => json!({"ok": v}),
        Err(message) => json!({"error": {"message": format!("{op}: {message}")}}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSA_PUB: &str = "MIIBCgKCAQEAv9kktYHqYI5Hj5Y7NS5XK2PzSAfu9at111m66K1wirtUgfOzCaxfYkuuQB3uQex1aD3l+wp6yAYGjFHqti7CE5d1lTXxU9AtYs/e2jh92+03dwzhUbiDXaGj4jUy3a3KmmE+J6GMOEFnvVGJG7scRdHu1HyQGQeBzZWOQJz/P850uiynCXX9vnEXqzO5tn9pnt3C2On+5STw0ZJbpWTCKdfZefwqlyTC9dpWDvi4s0Gbsbt6MZCfULQq39q3w83MZ3JdVGCxfG/6tXuvMqBbdGLoM47HR2sYfvEIo4nZ8PYFITvGrKMkPl0k1ObhTAHEAZDIueKFxRjTYiljMjmnAQIDAQAB";
    const RSA_PK8: &str = "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC/2SS1gepgjkePljs1LlcrY/NIB+71q3XXWbrorXCKu1SB87MJrF9iS65AHe5B7HVoPeX7CnrIBgaMUeq2LsITl3WVNfFT0C1iz97aOH3b7Td3DOFRuINdoaPiNTLdrcqaYT4noYw4QWe9UYkbuxxF0e7UfJAZB4HNlY5AnP8/znS6LKcJdf2+cRerM7m2f2me3cLY6f7lJPDRklulZMIp19l5/CqXJML12lYO+LizQZuxu3oxkJ9QtCrf2rfDzcxncl1UYLF8b/q1e68yoFt0YugzjsdHaxh+8Qijidnw9gUhO8asoyQ+XSTU5uFMAcQBkMi54oXFGNNiKWMyOacBAgMBAAECggEAVzudsPiPQXNAf74gUuis0zqB6v1MfyGFFjQjRFxkwXg3j1Vn0dyyjGf8JgRWC1zA7/LBSXp05NG0lYZD8q3lKnJikUOzZVhR4s3BN7mbJO7CmxvcskNzXBY9BgUJiXnWF4di8zNGJmg7Cv7xmI3mZ6ry7WRsQy1CLOA1jBS3DWQAdobIUY8I4CixywEXDIv+05wTwQqop0E8XBxB5WRxfsZ8rlJz6CiDWsBP1egR+L7xEi0Cj+RnYiwi+mmgdGWRg4ftyS6xgR+FO0pyWbfnQXZGLq8mSoe7mzC/y5aNublVK/ceqRFDjlo89jXw9xySmpJW2NvYTwI1Srvds+R6nwKBgQD6CoigbWMm/HDTPopVuzMYvk1+s3kcO2oDX5RVXdHzPXdM8XkVKf+CIU43wzmWLXf0CFncxFvstNWj0s5uPo5uvyIJJFpTpw7CNxtwXWqZRhlgvW88VmrRBloNfiCPrOSYMCWI6cbS7GWHk2JcwPqS/maxbtskd16WnTKCcX5dfwKBgQDEa5UseqKnIzwdegInbJ0uhZsDrz7zM3DaUb3aAYEhJvC/1IsWm0uzdoL20uWnC4muYx8Fj2fp0imgZt84pM4xLZLvO4LI5JtN0UijCK7+5I11kTcwYWgSSVDti0k1pU1Mtx0vkDgDpVwB4lQlYgYtB4DUA0/F3A34BKRVk4M7fwKBgB2p6EGYGJ5EbqIxFjpLhilWUl1Ir4KQ7efOjAuAoOrJjQC82Q+QtvZaOyxUZd2SMVvrLbYKOZmgboQL+1+K3lz3IfORs4GfHRofbYrsGjxq25Hlh2/SFMQNi9tz7ir9aPVBKQLQMT1OvFDURGQapN+RfN9p9meiHb+ikJ53FnXDAoGBAIyKHZZRuQe14lGE9v2WbXDYD+42+eexqtHLIvhVkztRTXk0bsCiJSwnY9WPWNqJGt439f5YeZWH1lj6wkUZRp9RaIzcG9v6FlU7palhkTS3+69fVAm96GUNKGaO/rbvq7t0WeGqYynYq1h9xI9q79hQQhFwvSdpKGWJjLvebEupAoGAcGmRn+r6KQfVCuJOZUaz2T88MADo16cVPDwbBQcrp7fTHF6L+648rg1nuYvRAtiQ18pjsfsJoEaPmvXlhvGWmqcP72lkFutdpGwWkvjvizPyTrO722s1H7wrzDeKnA5JEE7bbNDtVWSkBqIBsu18iXwZCHU0OIc636cUrvIcUao=";
    const RSA1024_PUB: &str = "MIGJAoGBAMSDoo62KBGwT4D0wEgugyXd1ke2DjbY0lvQXCDyRNH4+S6lBabwkSfjXaCbAKnpph50sM3so5Mg8CwNPp234nYQR5SgunOvML+3rLerbx4gGFQA3LclvuS6/HLuErcsONAZzl3ESFqw59vqEjuti7bvoghZxoxB0edttp0eR+Z7AgMBAAE=";
    const P256_PK8: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg+JRZS0uQ13FS9CMe3MCI4ASDJ9GI2xc76weyKi5wj7mhRANCAAQa1HtqF0NqakogfiUA1qp1wrSmUQTZJU8AA1mi6pcj3AO7fkGYGLMJIVneg5+6rQkLEtVyTd6F90iiIEhg6CdK";
    const P256_PUB: &str = "BBrUe2oXQ2pqSiB+JQDWqnXCtKZRBNklTwADWaLqlyPcA7t+QZgYswkhWd6Dn7qtCQsS1XJN3oX3SKIgSGDoJ0o=";
    const P384_PK8: &str = "MIG2AgEAMBAGByqGSM49AgEGBSuBBAAiBIGeMIGbAgEBBDCOB94/r9zGBUFSOCr2O7PHT/JnhRDnfOvX0lsBX+9Rj/9qcyCCsCNIEOB9kj31W4ihZANiAASFA/OJHf67ShBrpnwbUnmg0h30hW6H0GHdvn/4A2tgdR5rVjBE3f/FOMTHGhbSx+Egi3ITTrviKwYV9M13oWEYFgpLB/18XWeKxufV2GmB1CWvgNOpYCQe5hGfBLDiKrU=";
    const P384_PUB: &str = "BIUD84kd/rtKEGumfBtSeaDSHfSFbofQYd2+f/gDa2B1HmtWMETd/8U4xMcaFtLH4SCLchNOu+IrBhX0zXehYRgWCksH/XxdZ4rG59XYaYHUJa+A06lgJB7mEZ8EsOIqtQ==";
    const ED_PK8: &str = "MC4CAQAwBQYDK2VwBCIEIMRX6QdDvYJA3o4X4X6s5dhmZ312oTijIeIm/fOIEQHT";
    const ED_PUB: &str = "nECFoPPomyJClBclbKBJ/oGJ+bYPALu1HctuYq9CNQA=";

    /// Runs the op against a scratch guest memory holding `fields`; each `(name, bytes)` is
    /// passed as a `{ptr, len}` reference. Returns the reply, the memory and the args.
    fn run(op: &str, extra: JsonValue, fields: &[(&str, &[u8])]) -> (JsonValue, Vec<u8>, JsonValue) {
        let mut mem = vec![0u8; 16];
        let mut args = extra;
        for (name, b) in fields {
            let ptr = mem.len();
            mem.extend_from_slice(b);
            args[*name] = json!({"ptr": ptr, "len": b.len()});
        }
        let reply = sync_op(op, &args, &mut mem).unwrap();
        (reply, mem, args)
    }

    fn d(s: &str) -> Vec<u8> {
        base64::decode(s).unwrap()
    }

    fn h(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn caps_and_unknown() {
        let caps = sync_op("crypto/caps", &json!({}), &mut []).unwrap();
        assert_eq!(caps["ok"]["version"], 2);
        assert!(caps["ok"]["ops"]["verify"].as_array().unwrap().contains(&json!("EdDSA")));
        assert!(sync_op("now", &json!({}), &mut []).is_none());
        let (r, ..) = run("crypto/verify", json!({"alg": "XX"}), &[]);
        assert!(r.get("error").is_some());
    }

    #[test]
    fn sign_verify_roundtrip_all_algs() {
        let msg = b"hello host crypto";
        for (algs, pk8, public) in [
            (&["RS256", "RS384", "RS512", "PS256", "PS384", "PS512"][..], RSA_PK8, RSA_PUB),
            (&["ES256"][..], P256_PK8, P256_PUB),
            (&["ES384"][..], P384_PK8, P384_PUB),
            (&["EdDSA"][..], ED_PK8, ED_PUB),
        ] {
            for alg in algs {
                let (r, ..) = run("crypto/sign", json!({"alg": alg}), &[("key", &d(pk8)), ("msg", msg)]);
                let sig = d(r["ok"].as_str().unwrap_or_else(|| panic!("{alg}: {r}")));
                let (r, ..) = run(
                    "crypto/verify",
                    json!({"alg": alg}),
                    &[("key", &d(public)), ("msg", msg), ("sig", &sig)],
                );
                assert_eq!(r, json!({"ok": true}), "{alg}");
                let (r, ..) = run(
                    "crypto/verify",
                    json!({"alg": alg}),
                    &[("key", &d(public)), ("msg", b"other"), ("sig", &sig)],
                );
                assert_eq!(r, json!({"ok": false}), "{alg} wrong msg");
            }
        }
    }

    #[test]
    fn pubkey_matches_fixtures() {
        for (alg, pk8, public) in [("ES256", P256_PK8, P256_PUB), ("ES384", P384_PK8, P384_PUB), ("EdDSA", ED_PK8, ED_PUB)] {
            let (r, ..) = run("crypto/pubkey", json!({"alg": alg}), &[("key", &d(pk8))]);
            assert_eq!(d(r["ok"].as_str().unwrap()), d(public), "{alg}");
        }
    }

    #[test]
    fn rsa_pkcs1_signature_is_deterministic() {
        // Known-answer: RS256 over the shared message (also checked by the V8 host tests).
        let (r, ..) = run(
            "crypto/sign",
            json!({"alg": "RS256"}),
            &[("key", &d(RSA_PK8)), ("msg", &d("aGVsbG8gaG9zdCBjcnlwdG8="))],
        );
        assert_eq!(
            r["ok"].as_str().unwrap(),
            "T92XBvZfQMzizfrWucDaFhvCDsk5I4AVTfGEKdAHuSSfJ5588rH5D9s6liMp6I4gJACrPOoFn3cixRWEOFKw23+Hex8Vxnc6oeMF4xOpbsD7x2WSpete4ihJT4MwsyNujngfUyDcFjoiflFBRUBgIxu59p2NfXTnbegHZD03pODfHjzl3EHFuyDmRTvOyN+QJbPFhFAg9ZVUITcptUMBvvYMQXGx6qvtPAXUn++tq0eBXUbD6RCq4m6IxP1IRchnXyVQ/pDvVjox8voETPcXfyfQZKzpGgKNwViejxIMnAv0asyvvAcH7elRh93Vf/1ZOBxG3pR5eRUbQJmBfwKzuA=="
        );
    }

    #[test]
    fn ed25519_known_answer_and_rejections() {
        // RFC 8032 test 1 (empty message), as raw key + signature.
        let public = h("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let sig = h("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b");
        let (r, ..) = run("crypto/verify", json!({"alg": "EdDSA"}), &[("key", &public), ("msg", b""), ("sig", &sig)]);
        assert_eq!(r, json!({"ok": true}));
        // Unusable keys are errors, not false.
        let (r, ..) = run("crypto/verify", json!({"alg": "RS256"}), &[("key", &d(P256_PUB)), ("msg", b"m"), ("sig", b"s")]);
        assert!(r.get("error").is_some());
        let (r, ..) = run("crypto/verify", json!({"alg": "EdDSA"}), &[("key", &d(P256_PUB)), ("msg", b"m"), ("sig", b"s")]);
        assert!(r.get("error").is_some());
    }

    #[test]
    fn rsa_key_size_policy() {
        let (r, ..) = run("crypto/verify", json!({"alg": "RS256"}), &[("key", &d(RSA1024_PUB)), ("msg", b"m"), ("sig", &[0u8; 128])]);
        assert!(r["error"]["message"].as_str().unwrap().contains("1024"), "{r}");
    }

    #[test]
    fn aes_gcm_nist_test_case_4_in_place() {
        let key = h("feffe9928665731c6d6a8f9467308308");
        let iv = h("cafebabefacedbaddecaf888");
        let aad = h("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let pt = h("d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39");
        let ct = h("42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091");
        let tag = h("5bc94fbc3221a5db94fae95ae7121a47");
        let fields = [("key", &key[..]), ("nonce", &iv[..]), ("aad", &aad[..])];
        let mut f: Vec<(&str, &[u8])> = fields.to_vec();
        f.push(("data", &pt));
        let (r, mem, args) = run("crypto/aead", json!({"alg": "A128GCM", "dir": "seal"}), &f);
        assert_eq!(d(r["ok"]["tag"].as_str().unwrap()), tag);
        let (p, l) = (args["data"]["ptr"].as_u64().unwrap() as usize, args["data"]["len"].as_u64().unwrap() as usize);
        assert_eq!(&mem[p..p + l], &ct[..]);
        // open: good tag decrypts in place, bad tag leaves data untouched.
        let mut f: Vec<(&str, &[u8])> = fields.to_vec();
        f.push(("data", &ct));
        f.push(("tag", &tag));
        let (r, mem, args) = run("crypto/aead", json!({"alg": "A128GCM", "dir": "open"}), &f);
        assert_eq!(r, json!({"ok": true}));
        let p = args["data"]["ptr"].as_u64().unwrap() as usize;
        assert_eq!(&mem[p..p + ct.len()], &pt[..]);
        let mut bad = tag.clone();
        bad[0] ^= 1;
        let mut f: Vec<(&str, &[u8])> = fields.to_vec();
        f.push(("data", &ct));
        f.push(("tag", &bad));
        let (r, mem, args) = run("crypto/aead", json!({"alg": "A128GCM", "dir": "open"}), &f);
        assert_eq!(r, json!({"ok": false}));
        let p = args["data"]["ptr"].as_u64().unwrap() as usize;
        assert_eq!(&mem[p..p + ct.len()], &ct[..]);
        // bad nonce length / key length are errors
        let (r, ..) = run("crypto/aead", json!({"alg": "A256GCM", "dir": "seal"}), &[("key", &key), ("nonce", &iv), ("aad", b""), ("data", b"x")]);
        assert!(r.get("error").is_some());
    }

    #[test]
    fn digest_and_hmac_known_answers() {
        let (r, ..) = run("crypto/digest", json!({"alg": "SHA-256"}), &[("data", b"abc")]);
        assert_eq!(d(r["ok"].as_str().unwrap()), h("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        let (r, ..) = run("crypto/digest", json!({"alg": "SHA-1"}), &[("data", b"abc")]);
        assert_eq!(d(r["ok"].as_str().unwrap()), h("a9993e364706816aba3e25717850c26c9cd0d89d"));
        // RFC 4231 test case 2.
        let (r, ..) = run("crypto/hmac", json!({"alg": "SHA-256"}), &[("key", b"Jefe"), ("data", b"what do ya want for nothing?")]);
        assert_eq!(d(r["ok"].as_str().unwrap()), h("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"));
        // Empty key = zero-padded key (the V8 host maps it to one zero byte).
        let (a, ..) = run("crypto/hmac", json!({"alg": "SHA-256"}), &[("key", b""), ("data", b"x")]);
        let (b, ..) = run("crypto/hmac", json!({"alg": "SHA-256"}), &[("key", &[0u8]), ("data", b"x")]);
        assert_eq!(a, b);
    }

    #[test]
    fn out_of_bounds_ref_is_an_error() {
        let mut mem = vec![0u8; 8];
        let r = sync_op("crypto/digest", &json!({"alg": "SHA-256", "data": {"ptr": 4, "len": 100}}), &mut mem).unwrap();
        assert!(r.get("error").is_some());
        let r = sync_op("crypto/digest", &json!({"alg": "SHA-256", "data": {"ptr": u64::MAX, "len": 2}}), &mut mem).unwrap();
        assert!(r.get("error").is_some());
    }

    #[test]
    fn modulus_bits() {
        assert_eq!(rsa_modulus_bits(&[0x30, 0x07, 0x02, 0x02, 0x00, 0xc0, 0x02, 0x01, 0x03]), Some(8));
        assert_eq!(rsa_modulus_bits(&[0x31]), None);
    }

    #[test]
    fn oversized_input_is_rejected() {
        let mut mem = vec![0u8; 16];
        let args = json!({"alg": "SHA-256", "data": {"ptr": 0, "len": (MAX_INPUT_BYTES + 1) as u64}});
        let r = sync_op("crypto/digest", &args, &mut mem).unwrap();
        assert!(r["error"]["message"].as_str().unwrap().contains("too large"), "{r}");
    }
}
