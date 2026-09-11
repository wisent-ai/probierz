//! The Ed25519 keys behind a receipt: parsing the signing and verifying keys, the
//! public DER, signing a payload, and the identifier a signed payload gets.

use crate::evidence::*;
use serde_json::json;

pub(crate) fn signing_key(input: &[u8]) -> Result<SigningKey, Failure> {
    let text = String::from_utf8_lossy(input).trim().to_string();
    let key = if text.contains("BEGIN") {
        SigningKey::from_pkcs8_pem(&text)
    } else {
        let der = BASE64
            .decode(text)
            .map_err(|error| Failure::config("evidence.receipt", error.to_string()))?;
        SigningKey::from_pkcs8_der(&der)
    };
    key.map_err(|error| {
        Failure::config(
            "evidence.receipt",
            format!("evidence private key must be Ed25519: {error}"),
        )
    })
}

pub(crate) fn verifying_key(input: &[u8]) -> Result<VerifyingKey, Failure> {
    let text = String::from_utf8_lossy(input).trim().to_string();
    if text.contains("BEGIN") {
        VerifyingKey::from_public_key_pem(&text)
    } else {
        VerifyingKey::from_public_key_der(input)
    }
    .map_err(|error| {
        Failure::config(
            "evidence.verify_receipt",
            format!("receipt public key must be Ed25519: {error}"),
        )
    })
}

pub(crate) fn public_der(key: &VerifyingKey) -> Result<Vec<u8>, Failure> {
    Ok(key
        .to_public_key_der()
        .map_err(|error| Failure::config("evidence.receipt", error.to_string()))?
        .as_bytes()
        .to_vec())
}

pub(crate) fn sign_payload(payload: &Value, private_key: &[u8]) -> Result<Value, Failure> {
    let private = signing_key(private_key)?;
    let public = private.verifying_key();
    let canonical_payload = canonical(payload);
    let signature = private.sign(canonical_payload.as_bytes());
    let pem = public
        .to_public_key_pem(LineEnding::LF)
        .map_err(|error| Failure::config("evidence.receipt", error.to_string()))?;
    Ok(json!({
        "algorithm": "Ed25519",
        "publicKeyFingerprintSha256": sha256_bytes(&public_der(&public)?),
        "publicKeyPem": pem,
        "payloadSha256": sha256_bytes(canonical_payload.as_bytes()),
        "signature": BASE64.encode(signature.to_bytes()),
    }))
}

pub(crate) fn signed_evidence_id(payload: &Value, signing: &Value) -> String {
    let signature = signing
        .get("signature")
        .and_then(Value::as_str)
        .unwrap_or_default();
    sha256_bytes(format!("{}\n{signature}", canonical(payload)).as_bytes())[..24].to_string()
}
