// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! RSA signing of the `M-XXXX` product folder — produces the ETS `.signature`.
//!
//! This is the step ETS performs internally via the closed-source
//! `Knx.Ets.XmlSigning.dll` `SignDirectory` method. `knx-rs-prod` **never ships
//! a key**: the caller supplies an RSA private key that they extracted from
//! their *own* licensed ETS installation. Reimplementing the signing *format*
//! for interoperability, and signing with a key you are licensed to use, is the
//! supported path under the software-interoperability rules — this module is the
//! *algorithm*, not the key.
//!
//! This is distinct from [`crate::sign`], which patches the registration MD5
//! `Hash` attribute + fingerprint (an *input* to signing, not the signature).
//!
//! # Algorithm
//!
//! Reverse-engineered from `Knx.Ets.XmlSigning.DirectorySigner` and verified
//! byte-exact against a real ETS `.signature`: for each file under the folder
//! (recursive, excluding `.signature`) build `"<relpath>:<Base64(SHA1(bytes))>"`,
//! sort by `<relpath>`, join with `,`, then RSA-PKCS#1-v1.5 sign `SHA1(UTF-8(…))`
//! — see `canonical_message`. [`verify_directory_signature`] checks output
//! against a known-good sample. Tracking: <https://github.com/metaneutrons/knx-rs/issues/9>.

use std::path::{Path, PathBuf};

use base64::prelude::{BASE64_STANDARD, Engine as _};
use rsa::RsaPrivateKey;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::{Signature, SigningKey as RsaSigningKey, VerifyingKey};
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer, Verifier};
use sha1::{Digest, Sha1};

use crate::error::KnxprodError;

/// UTF-8 byte-order mark prefixed to the base64 `.signature` payload.
///
/// Confirmed by unzipping a real ETS `.knxprod`: each `.signature` file is
/// `BOM || base64(rsa_signature)` with no trailing newline.
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// An RSA private key supplied by the caller, used to sign the product folder.
///
/// Load it from the caller's own ETS key material via [`SigningKey::from_path`]
/// (auto-detects PEM vs. .NET `<RSAKeyValue>` XML), or the explicit constructors.
pub struct SigningKey {
    inner: RsaPrivateKey,
}

impl SigningKey {
    /// Load a key from a PKCS#8 or PKCS#1 PEM string.
    ///
    /// # Errors
    ///
    /// Returns [`KnxprodError::Signing`] if the PEM is not a valid RSA key.
    pub fn from_pem(pem: &str) -> Result<Self, KnxprodError> {
        let inner = RsaPrivateKey::from_pkcs8_pem(pem)
            .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
            .map_err(|e| KnxprodError::Signing(format!("invalid PEM RSA key: {e}")))?;
        Ok(Self { inner })
    }

    /// Load a key from the .NET `<RSAKeyValue>` XML that `RSA.ToXmlString(true)`
    /// emits (the zero-conversion export from an ETS install).
    ///
    /// # Errors
    ///
    /// Returns [`KnxprodError::Signing`] if a component is missing or malformed.
    pub fn from_dotnet_xml(xml: &str) -> Result<Self, KnxprodError> {
        Ok(Self {
            inner: dotnet_xml_to_rsa(xml)?,
        })
    }

    /// Load a key from a file, sniffing PEM vs. .NET XML by content.
    ///
    /// # Errors
    ///
    /// Returns [`KnxprodError`] if the file cannot be read or parsed.
    pub fn from_path(path: &Path) -> Result<Self, KnxprodError> {
        let s = std::fs::read_to_string(path).map_err(|e| KnxprodError::io(path, e))?;
        if s.contains("<RSAKeyValue") {
            Self::from_dotnet_xml(&s)
        } else {
            Self::from_pem(&s)
        }
    }

    /// Sign `msg` with RSASSA-PKCS1-v1_5 over SHA-1 — the scheme ETS uses.
    #[must_use]
    pub fn sign_message(&self, msg: &[u8]) -> Vec<u8> {
        RsaSigningKey::<Sha1>::new(self.inner.clone())
            .sign(msg)
            .to_vec()
    }

    /// The corresponding public key (for [`verify_directory_signature`]).
    #[must_use]
    pub fn public_key(&self) -> rsa::RsaPublicKey {
        self.inner.to_public_key()
    }
}

/// Parse a .NET `<RSAKeyValue>` XML blob into an [`RsaPrivateKey`].
#[allow(clippy::expect_used, clippy::many_single_char_names)]
fn dotnet_xml_to_rsa(xml: &str) -> Result<RsaPrivateKey, KnxprodError> {
    use rsa::BigUint;

    let field = |tag: &str| -> Result<BigUint, KnxprodError> {
        let re = regex::Regex::new(&format!(r"<{tag}>([^<]+)</{tag}>")).expect("valid regex");
        let cap = re
            .captures(xml)
            .ok_or_else(|| KnxprodError::Signing(format!("RSAKeyValue missing <{tag}>")))?;
        let bytes = BASE64_STANDARD
            .decode(cap[1].trim())
            .map_err(|e| KnxprodError::Signing(format!("bad base64 in <{tag}>: {e}")))?;
        Ok(BigUint::from_bytes_be(&bytes))
    };

    let n = field("Modulus")?;
    let e = field("Exponent")?;
    let d = field("D")?;
    let p = field("P")?;
    let q = field("Q")?;

    RsaPrivateKey::from_components(n, e, d, vec![p, q])
        .map_err(|e| KnxprodError::Signing(format!("invalid RSA components: {e}")))
}

/// The exact byte string ETS feeds into RSASSA-PKCS1-v1_5 for a product folder.
///
/// Reverse-engineered from `Knx.Ets.XmlSigning.DirectorySigner` and verified
/// byte-exact against a real ETS `.signature` (`reference/leakage.knxprod`).
/// For every file under the folder (recursively, excluding any `.signature`),
/// form `"<relpath>:<Base64(SHA1(bytes))>"`; sort by `<relpath>`; join with `,`.
/// ETS then signs `SHA1(UTF-8(that))`.
///
/// `<relpath>` is the path relative to the folder, `\`-separated (ETS/Windows).
/// ETS sorts with `StringComparer.InvariantCulture`; ordinal ordering is used
/// here, which matches for the common flat layout
/// (`Catalog.xml`/`Hardware.xml`/app`[/Baggages.xml]`). Deeply-nested
/// `Baggages\…` collation is not yet re-verified.
fn canonical_message(manu_dir: &Path) -> Result<Vec<u8>, KnxprodError> {
    let mut entries: Vec<(String, String)> = Vec::new();
    collect_entries(manu_dir, &mut Vec::new(), &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let joined = entries
        .iter()
        .map(|(rel, hash)| format!("{rel}:{hash}"))
        .collect::<Vec<_>>()
        .join(",");
    Ok(joined.into_bytes())
}

/// Recursively collect `(relpath, Base64(SHA1(bytes)))` for each file under
/// `dir`, using a `\`-joined relative path and skipping `.signature` files.
fn collect_entries(
    dir: &Path,
    rel: &mut Vec<String>,
    out: &mut Vec<(String, String)>,
) -> Result<(), KnxprodError> {
    // Propagate per-entry errors: a dropped entry would silently produce an
    // incomplete (ETS-rejected) signature that is hard to debug.
    let mut items: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| KnxprodError::io(dir, e))? {
        items.push(entry.map_err(|e| KnxprodError::io(dir, e))?.path());
    }
    items.sort();
    for p in items {
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| KnxprodError::InvalidStructure("non-UTF-8 filename".into()))?
            .to_owned();
        if p.is_dir() {
            rel.push(name);
            collect_entries(&p, rel, out)?;
            rel.pop();
        } else if !name.ends_with(".signature") {
            let bytes = std::fs::read(&p).map_err(|e| KnxprodError::io(&p, e))?;
            let hash = BASE64_STANDARD.encode(<Sha1 as Digest>::digest(&bytes));
            rel.push(name);
            out.push((rel.join("\\"), hash));
            rel.pop();
        }
    }
    Ok(())
}

/// Sign the `M-XXXX` product folder and write the sibling `M-XXXX.signature`.
///
/// The `.signature` is placed next to the folder (i.e. at the archive root),
/// matching the ETS layout.
///
/// # Errors
///
/// Returns [`KnxprodError`] if the folder cannot be read or the file written.
pub fn sign_directory(manu_dir: &Path, key: &SigningKey) -> Result<PathBuf, KnxprodError> {
    let msg = canonical_message(manu_dir)?;
    let sig = key.sign_message(&msg);

    let folder = manu_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| KnxprodError::InvalidStructure("invalid manufacturer folder name".into()))?;
    let parent = manu_dir.parent().unwrap_or_else(|| Path::new("."));
    let sig_path = parent.join(format!("{folder}.signature"));

    write_signature_file(&sig_path, &sig)?;
    Ok(sig_path)
}

/// Write a raw RSA signature as an ETS `.signature` file (`BOM || base64(sig)`).
///
/// # Errors
///
/// Returns [`KnxprodError`] if the file cannot be written.
pub fn write_signature_file(path: &Path, signature: &[u8]) -> Result<(), KnxprodError> {
    let mut out = Vec::with_capacity(UTF8_BOM.len() + signature.len().div_ceil(3) * 4);
    out.extend_from_slice(&UTF8_BOM);
    out.extend_from_slice(BASE64_STANDARD.encode(signature).as_bytes());
    std::fs::write(path, out).map_err(|e| KnxprodError::io(path, e))
}

/// Verify an existing `.signature` against a folder and public key.
///
/// Used to validate output against a known-good ETS sample.
///
/// # Errors
///
/// Returns [`KnxprodError`] if the signature file or folder cannot be read or
/// the signature bytes are malformed.
pub fn verify_directory_signature(
    manu_dir: &Path,
    sig_file: &Path,
    public_key: &rsa::RsaPublicKey,
) -> Result<bool, KnxprodError> {
    let raw = std::fs::read(sig_file).map_err(|e| KnxprodError::io(sig_file, e))?;
    // ETS `.signature` files are `BOM || base64(sig)`; require the BOM so a
    // malformed file is rejected rather than silently mis-parsed.
    let b64 = raw
        .strip_prefix(&UTF8_BOM)
        .ok_or_else(|| KnxprodError::Signing("signature file missing UTF-8 BOM".into()))?;
    let sig_bytes = BASE64_STANDARD
        .decode(b64)
        .map_err(|e| KnxprodError::Signing(format!("bad base64 signature: {e}")))?;
    let sig = Signature::try_from(sig_bytes.as_slice())
        .map_err(|e| KnxprodError::Signing(format!("bad signature bytes: {e}")))?;

    let msg = canonical_message(manu_dir)?;
    Ok(VerifyingKey::<Sha1>::new(public_key.clone())
        .verify(&msg, &sig)
        .is_ok())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Build a `.NET` `<RSAKeyValue>` XML string from an [`RsaPrivateKey`].
    fn to_dotnet_xml(key: &RsaPrivateKey) -> String {
        use rsa::traits::{PrivateKeyParts, PublicKeyParts};
        let b64 = |v: &rsa::BigUint| BASE64_STANDARD.encode(v.to_bytes_be());
        let primes = key.primes();
        format!(
            "<RSAKeyValue><Modulus>{}</Modulus><Exponent>{}</Exponent>\
             <P>{}</P><Q>{}</Q><D>{}</D></RSAKeyValue>",
            b64(key.n()),
            b64(key.e()),
            b64(&primes[0]),
            b64(&primes[1]),
            b64(key.d()),
        )
    }

    #[test]
    fn canonical_message_matches_ets_format() {
        let dir = tempfile::tempdir().unwrap();
        let manu = dir.path().join("M-00FA");
        std::fs::create_dir_all(manu.join("Sub")).unwrap();
        std::fs::write(manu.join("Catalog.xml"), b"AAA").unwrap();
        std::fs::write(manu.join("Hardware.xml"), b"BBBB").unwrap();
        std::fs::write(manu.join("Sub").join("Baggage.bin"), b"CCCCC").unwrap();
        // A pre-existing .signature must be excluded.
        std::fs::write(manu.join("M-00FA.signature"), b"x").unwrap();

        let msg = String::from_utf8(canonical_message(&manu).unwrap()).unwrap();
        let h = |b: &[u8]| BASE64_STANDARD.encode(<Sha1 as Digest>::digest(b));
        // Entries: "<relpath>:<Base64(SHA1(bytes))>" joined by ',', sorted by relpath
        // ('Catalog' < 'Hardware' < 'Sub\\Baggage.bin' ordinally).
        let expect = format!(
            "Catalog.xml:{},Hardware.xml:{},Sub\\Baggage.bin:{}",
            h(b"AAA"),
            h(b"BBBB"),
            h(b"CCCCC"),
        );
        assert_eq!(msg, expect);
    }

    #[test]
    fn sign_then_verify_roundtrips_and_writes_bom_base64() {
        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 1024).unwrap();
        let key = SigningKey {
            inner: priv_key.clone(),
        };

        let dir = tempfile::tempdir().unwrap();
        let manu = dir.path().join("M-00FA");
        std::fs::create_dir_all(&manu).unwrap();
        std::fs::write(manu.join("Catalog.xml"), "<Catalog/>").unwrap();
        std::fs::write(manu.join("Hardware.xml"), "<Hardware/>").unwrap();

        let sig_path = sign_directory(&manu, &key).unwrap();

        // File format: UTF-8 BOM followed by base64 text.
        let raw = std::fs::read(&sig_path).unwrap();
        assert_eq!(&raw[..3], &UTF8_BOM);
        assert!(sig_path.file_name().unwrap().to_str().unwrap() == "M-00FA.signature");

        // A pre-existing .signature must not perturb the signed message.
        let ok = verify_directory_signature(&manu, &sig_path, &priv_key.to_public_key()).unwrap();
        assert!(ok, "signature should verify against its own public key");
    }

    #[test]
    fn dotnet_xml_key_signs_identically_to_source_key() {
        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 1024).unwrap();

        let xml = to_dotnet_xml(&priv_key);
        let parsed = SigningKey::from_dotnet_xml(&xml).unwrap();

        let direct = SigningKey { inner: priv_key };
        let msg = b"the quick brown fox";
        assert_eq!(
            parsed.sign_message(msg),
            direct.sign_message(msg),
            "key parsed from .NET XML must reproduce the same signature"
        );
    }

    /// End-to-end proof against real ETS output: reproduce the `M-0083.signature`
    /// inside `reference/leakage.knxprod` byte-for-byte. Needs the ETS signing key
    /// (never committed) at `$ETS_KEY`; skipped when unset.
    #[test]
    #[ignore = "needs a signing key at $ETS_KEY"]
    fn reproduces_reference_leakage_signature() {
        use std::io::Read;
        let Ok(key_path) = std::env::var("ETS_KEY") else {
            return;
        };
        let key = SigningKey::from_path(Path::new(&key_path)).unwrap();

        let file = std::fs::File::open("reference/leakage.knxprod").unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manu = dir.path().join("M-0083");
        std::fs::create_dir_all(&manu).unwrap();
        let mut real_sig = Vec::new();
        for i in 0..zip.len() {
            let mut e = zip.by_index(i).unwrap();
            let name = e.name().to_string();
            if name == "M-0083.signature" {
                e.read_to_end(&mut real_sig).unwrap();
            } else if let Some(rest) = name.strip_prefix("M-0083/") {
                if rest.is_empty() || rest.ends_with('/') {
                    continue;
                }
                let out = manu.join(rest);
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                let mut buf = Vec::new();
                e.read_to_end(&mut buf).unwrap();
                std::fs::write(out, buf).unwrap();
            }
        }

        let sig_path = sign_directory(&manu, &key).unwrap();
        let ours = std::fs::read(&sig_path).unwrap();
        assert_eq!(
            ours, real_sig,
            "our .signature must match ETS byte-for-byte"
        );
    }
}
