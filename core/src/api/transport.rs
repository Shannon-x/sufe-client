//! Wire-compatible client for the adjacent `sufe-middleware-rs` Stealth-v1.
//! The protocol is HKDF-SHA256 -> AES-256-GCM and a separate HMAC path key.
//! Query, body and response always use independent random nonces.

use crate::error::{Result, XboardError};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use ring::{
    aead, hkdf, hmac,
    rand::{SecureRandom, SystemRandom},
};

pub struct Stealth {
    cipher: aead::LessSafeKey,
    path_key: hmac::Key,
}

impl std::fmt::Debug for Stealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stealth([redacted])")
    }
}

struct KeyLength(usize);
impl hkdf::KeyType for KeyLength {
    fn len(&self) -> usize {
        self.0
    }
}

pub(crate) fn derive(password: &str, salt: &[u8], info: &[u8]) -> Result<[u8; 64]> {
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(password.as_bytes());
    let context = [info];
    let mut out = [0; 64];
    prk.expand(&context, KeyLength(64))
        .map_err(|_| crypto_error())?
        .fill(&mut out)
        .map_err(|_| crypto_error())?;
    Ok(out)
}

pub(crate) fn crypto_error() -> XboardError {
    XboardError::Config("安全通道验证失败，请检查部署配置".into())
}

impl Stealth {
    pub fn new(password: &str) -> Result<Self> {
        if password.trim().is_empty() {
            return Err(crypto_error());
        }
        let mut keys = derive(password, b"xb-stealth-v1", b"xb-stealth-keystream/v1")?;
        let cipher = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, &keys[..32]).map_err(|_| crypto_error())?,
        );
        let path_key = hmac::Key::new(hmac::HMAC_SHA256, &keys[32..]);
        keys.fill(0);
        Ok(Self { cipher, path_key })
    }

    pub fn path(&self, real_path: &str) -> String {
        let tag = hmac::sign(&self.path_key, real_path.as_bytes());
        format!("/v2/{}", hex::encode(&tag.as_ref()[..16]))
    }

    fn nonce() -> Result<[u8; 12]> {
        let mut nonce = [0; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| crypto_error())?;
        Ok(nonce)
    }

    fn encrypt(&self, plain: &[u8], nonce: [u8; 12]) -> Result<String> {
        let mut body = plain.to_vec();
        self.cipher
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut body,
            )
            .map_err(|_| crypto_error())?;
        Ok(B64.encode(body))
    }

    pub fn prepare(
        &self,
        url: &mut url::Url,
        headers: &mut HeaderMap,
        body: Option<&serde_json::Value>,
    ) -> Result<Option<String>> {
        let query_nonce = Self::nonce()?;
        let body_nonce = Self::nonce()?;
        let path = self.path(url.path());
        let params: serde_json::Map<String, serde_json::Value> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), serde_json::Value::String(v.into_owned())))
            .collect();
        url.set_path(&path);
        url.set_query(None);
        if !params.is_empty() {
            let ciphertext = self.encrypt(&serde_json::to_vec(&params)?, query_nonce)?;
            url.query_pairs_mut().append_pair("q", &ciphertext);
        }
        headers.insert(
            "x-request-id",
            HeaderValue::from_str(&B64.encode(query_nonce)).map_err(|_| crypto_error())?,
        );
        headers.insert(
            "x-request-body-id",
            HeaderValue::from_str(&B64.encode(body_nonce)).map_err(|_| crypto_error())?,
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        body.map(|v| self.encrypt(&serde_json::to_vec(v)?, body_nonce))
            .transpose()
    }

    pub fn decrypt_response(&self, nonce: &str, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let nonce: [u8; 12] = B64
            .decode(nonce.trim())
            .map_err(|_| crypto_error())?
            .try_into()
            .map_err(|_| crypto_error())?;
        let mut data = B64.decode(ciphertext).map_err(|_| crypto_error())?;
        let plain = self
            .cipher
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut data,
            )
            .map_err(|_| crypto_error())?;
        Ok(plain.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middleware_cross_language_vector() {
        let crypto = Stealth::new("client-fixture-password").unwrap();
        // Independently generated with Python cryptography's HKDF/AESGCM.
        assert_eq!(
            crypto.path("/api/v1/user/info"),
            "/v2/fdcbf36a9ec86b04f098990af8c27f62"
        );
        assert_eq!(
            crypto
                .encrypt(
                    br#"{"email":"a@b.c"}"#,
                    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
                )
                .unwrap(),
            "lRF34w5Sfk/7f/jhiga3BKDOxfoZ6hocxulfaC5nUIzV"
        );
    }

    #[test]
    fn query_and_body_never_reuse_nonce_or_leak_plaintext() {
        let crypto = Stealth::new("test-password").unwrap();
        let mut url =
            url::Url::parse("https://panel.example/api/v1/user/order/check?trade_no=sensitive")
                .unwrap();
        let mut headers = HeaderMap::new();
        let body = crypto
            .prepare(
                &mut url,
                &mut headers,
                Some(&serde_json::json!({"secret":"value"})),
            )
            .unwrap()
            .unwrap();
        assert_ne!(headers["x-request-id"], headers["x-request-body-id"]);
        assert!(!url.as_str().contains("sensitive"));
        assert!(!body.contains("value"));
        assert!(crypto.decrypt_response("broken", body.as_bytes()).is_err());
        assert!(Stealth::new("other")
            .unwrap()
            .decrypt_response(
                headers["x-request-body-id"].to_str().unwrap(),
                body.as_bytes()
            )
            .is_err());
        assert_eq!(
            crypto
                .decrypt_response(
                    headers["x-request-body-id"].to_str().unwrap(),
                    body.as_bytes()
                )
                .unwrap(),
            br#"{"secret":"value"}"#
        );
    }
}
