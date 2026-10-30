//! WorkBuddy 加密凭据（`$wbEncrypted` 信封）解析与解密。
//!
//! 新版客户端把 `access_token` 存成字段加密信封：
//! ```json
//! { "$wbEncrypted": 1, "envelope": "<base64>" }
//! ```
//! `envelope` 解码后是一个 JSON 对象，键集合**恰好**为
//! `{suite, keyId, nonce, authTag, ciphertext}`：
//! - `suite == 1`（`sym-v1`），其它值一律拒绝；
//! - `keyId` 为 16 位小写 hex；
//! - `nonce` 12 字节、`authTag` 16 字节、`ciphertext` 任意长度（均 base64）。
//!
//! 解密参数：AES-256-GCM，`key = sha256(atRestSecretKey)`，
//! `keyId == sha256(key)` 的 hex 前 16 位；AAD 见 [`build_aad`]。
//!
//! `atRestSecretKey` 只能从客户端运行时取（见 `wb_runtime`），本模块只做纯计算，
//! 不联网、不起进程，全部函数可在单测里跑。
//!
//! ⚠️ 本机目前没有加密信封账号，**真实明文未实机验证**；单测只能覆盖
//! 结构校验与「用同一套参数加密再解密」的自洽性。

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// token 长度上限（参考实现取 32768）。
const TOKEN_LIMIT: usize = 32 * 1024;

/// 解密失败分类。`code()` 用于日志与界面（稳定字符串，勿随意改名）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// 信封结构不合法（键集合、字段类型、长度不符）。
    InvalidFormat(String),
    /// `suite` 不是受支持的 1。
    UnsupportedSuite(u64),
    /// `keyId` 与 `sha256(key)` 不匹配，通常是客户端换了密钥。
    KeyMismatch,
    /// 没找到 WorkBuddy 可执行文件。
    RuntimeNotFound(String),
    /// 找到了但跑不起来 / 取密钥失败。
    RuntimeUnavailable(String),
    /// 子进程超时。
    HelperTimeout,
    /// 子进程输出不是约定的 JSON。
    HelperProtocol(String),
    /// 解密本身失败（tag 校验不过）。
    DecryptFailed,
    /// 解出来的明文不像 token。
    TokenInvalid,
}

impl TokenError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidFormat(_) => "INVALID_FORMAT",
            Self::UnsupportedSuite(_) => "UNSUPPORTED_ENVELOPE",
            Self::KeyMismatch => "KEY_MISMATCH",
            Self::RuntimeNotFound(_) => "RUNTIME_NOT_FOUND",
            Self::RuntimeUnavailable(_) => "RUNTIME_UNAVAILABLE",
            Self::HelperTimeout => "HELPER_TIMEOUT",
            Self::HelperProtocol(_) => "HELPER_PROTOCOL",
            Self::DecryptFailed => "DECRYPT_FAILED",
            Self::TokenInvalid => "TOKEN_INVALID",
        }
    }

    /// 面向用户的一句话说明（不含任何密钥材料）。
    pub fn message(&self) -> String {
        match self {
            Self::InvalidFormat(detail) => format!("凭据信封结构不合法：{detail}"),
            Self::UnsupportedSuite(suite) => {
                format!("不支持的凭据加密套件 suite={suite}（当前仅支持 1）")
            }
            Self::KeyMismatch => "凭据密钥与本机客户端不一致，请重新登录该客户端".to_string(),
            Self::RuntimeNotFound(detail) => {
                format!("未找到 WorkBuddy 客户端，无法解密凭据：{detail}")
            }
            Self::RuntimeUnavailable(detail) => format!("客户端运行时不可用：{detail}"),
            Self::HelperTimeout => "读取凭据密钥超时".to_string(),
            Self::HelperProtocol(detail) => format!("读取凭据密钥失败：{detail}"),
            Self::DecryptFailed => "凭据解密失败（密钥或数据不匹配）".to_string(),
            Self::TokenInvalid => "解密结果不是合法的令牌".to_string(),
        }
    }
}

/// 解析后的信封。
#[derive(Debug, Clone)]
pub struct Envelope {
    pub key_id: String,
    pub nonce: [u8; 12],
    pub auth_tag: [u8; 16],
    pub ciphertext: Vec<u8>,
}

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn decode_exact(value: &Value, field: &str, want: Option<usize>) -> Result<Vec<u8>, TokenError> {
    let Some(text) = value.get(field).and_then(Value::as_str) else {
        return Err(TokenError::InvalidFormat(format!("缺少 {field}")));
    };
    let bytes = b64()
        .decode(text)
        .map_err(|e| TokenError::InvalidFormat(format!("{field} 不是合法 base64: {e}")))?;
    if let Some(want) = want {
        if bytes.len() != want {
            return Err(TokenError::InvalidFormat(format!(
                "{field} 长度应为 {want} 字节，实际 {}",
                bytes.len()
            )));
        }
    }
    if bytes.is_empty() {
        return Err(TokenError::InvalidFormat(format!("{field} 为空")));
    }
    Ok(bytes)
}

fn is_hex_key_id(text: &str) -> bool {
    text.len() == 16
        && text
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// 解析 `{"$wbEncrypted":1,"envelope":"<base64>"}`。
///
/// 任何结构问题都返回 [`TokenError::InvalidFormat`] / [`TokenError::UnsupportedSuite`]，
/// 不会 panic，也不会把信封内容写进错误文案。
pub fn parse_envelope(value: &Value) -> Result<Envelope, TokenError> {
    let obj = value
        .as_object()
        .ok_or_else(|| TokenError::InvalidFormat("access_token 不是对象".into()))?;
    // `$wbEncrypted` 必须是整数 1（字符串 "1" 不接受）。
    match obj.get("$wbEncrypted") {
        Some(Value::Number(n)) if n.as_u64() == Some(1) => {}
        Some(_) => {
            return Err(TokenError::InvalidFormat(
                "$wbEncrypted 必须为数字 1".into(),
            ))
        }
        None => return Err(TokenError::InvalidFormat("缺少 $wbEncrypted".into())),
    }
    let envelope_b64 = obj
        .get("envelope")
        .and_then(Value::as_str)
        .ok_or_else(|| TokenError::InvalidFormat("缺少 envelope".into()))?;
    let raw = b64()
        .decode(envelope_b64)
        .map_err(|e| TokenError::InvalidFormat(format!("envelope 不是合法 base64: {e}")))?;
    let inner: Value = serde_json::from_slice(&raw)
        .map_err(|e| TokenError::InvalidFormat(format!("envelope 不是 JSON: {e}")))?;
    let inner = inner
        .as_object()
        .ok_or_else(|| TokenError::InvalidFormat("envelope 内层不是对象".into()))?;

    const FIELDS: [&str; 5] = ["suite", "keyId", "nonce", "authTag", "ciphertext"];
    if inner.len() != FIELDS.len() || !FIELDS.iter().all(|f| inner.contains_key(*f)) {
        return Err(TokenError::InvalidFormat(
            "envelope 键集合必须恰好为 suite/keyId/nonce/authTag/ciphertext".into(),
        ));
    }

    let suite = inner
        .get("suite")
        .and_then(Value::as_u64)
        .ok_or_else(|| TokenError::InvalidFormat("suite 必须是整数".into()))?;
    if suite != 1 {
        return Err(TokenError::UnsupportedSuite(suite));
    }

    let key_id = inner
        .get("keyId")
        .and_then(Value::as_str)
        .ok_or_else(|| TokenError::InvalidFormat("keyId 必须是字符串".into()))?
        .to_string();
    if !is_hex_key_id(&key_id) {
        return Err(TokenError::InvalidFormat(
            "keyId 必须是 16 位小写 hex".into(),
        ));
    }

    let nonce: [u8; 12] = decode_exact(&Value::Object(inner.clone()), "nonce", Some(12))?
        .try_into()
        .map_err(|_| TokenError::InvalidFormat("nonce 长度错误".into()))?;
    let auth_tag: [u8; 16] = decode_exact(&Value::Object(inner.clone()), "authTag", Some(16))?
        .try_into()
        .map_err(|_| TokenError::InvalidFormat("authTag 长度错误".into()))?;
    let ciphertext = decode_exact(&Value::Object(inner.clone()), "ciphertext", None)?;

    Ok(Envelope {
        key_id,
        nonce,
        auth_tag,
        ciphertext,
    })
}

/// `key = sha256(atRestSecretKey)`。
pub fn derive_key(at_rest_secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(at_rest_secret.as_bytes());
    let out = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&out);
    key
}

fn hex16(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))[..16].to_string()
}

/// `keyId` 是否等于 `sha256(key)` 的 hex 前 16 位。
pub fn key_matches(key_id: &str, key: &[u8; 32]) -> bool {
    hex16(key) == key_id
}

/// 4 字节大端长度前缀 + 内容。
fn lp(out: &mut Vec<u8>, text: &[u8]) {
    out.extend_from_slice(&(text.len() as u32).to_be_bytes());
    out.extend_from_slice(text);
}

/// 构造 AAD。
///
/// ```text
/// "WB-AAD\0" | 0x01 | lp("WBEV1") | lp("sym-v1") | 00 00 00 01 | lp(keyId) | 02 00 00
/// ```
pub fn build_aad(key_id: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(48);
    out.extend_from_slice(b"WB-AAD\0");
    out.push(0x01);
    lp(&mut out, b"WBEV1");
    lp(&mut out, b"sym-v1");
    out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    lp(&mut out, key_id.as_bytes());
    out.extend_from_slice(&[0x02, 0x00, 0x00]);
    out
}

/// token 形态校验：可见 ASCII、无空白、长度受限。
fn looks_like_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= TOKEN_LIMIT
        && text
            .bytes()
            .all(|b| b.is_ascii_graphic() || b == b'=' || b == b'.' || b == b'_' || b == b'-')
}

/// 用 `atRestSecretKey` 解开信封，返回明文 token。
pub fn decrypt_envelope(envelope: &Envelope, at_rest_secret: &str) -> Result<String, TokenError> {
    let key = derive_key(at_rest_secret);
    if !key_matches(&envelope.key_id, &key) {
        return Err(TokenError::KeyMismatch);
    }
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| TokenError::InvalidFormat("密钥长度异常".into()))?;
    let aad = build_aad(&envelope.key_id);
    let mut body = envelope.ciphertext.clone();
    body.extend_from_slice(&envelope.auth_tag);
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&envelope.nonce),
            Payload {
                msg: &body,
                aad: &aad,
            },
        )
        .map_err(|_| TokenError::DecryptFailed)?;
    let text = String::from_utf8(plaintext).map_err(|_| TokenError::TokenInvalid)?;
    if !looks_like_token(&text) {
        return Err(TokenError::TokenInvalid);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "fake-at-rest-secret";

    fn key_id_for(secret: &str) -> String {
        hex16(&derive_key(secret))
    }

    /// 用同一套参数加密，构造一个自洽信封（服务端真实密文无法在本机生成）。
    fn seal(secret: &str, token: &str) -> Value {
        let key = derive_key(secret);
        let key_id = hex16(&key);
        let nonce_bytes = [7u8; 12];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let aad = build_aad(&key_id);
        let mut sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: token.as_bytes(),
                    aad: &aad,
                },
            )
            .unwrap();
        let tag = sealed.split_off(sealed.len() - 16);
        let inner = serde_json::json!({
            "suite": 1,
            "keyId": key_id,
            "nonce": b64().encode(nonce_bytes),
            "authTag": b64().encode(tag),
            "ciphertext": b64().encode(sealed),
        });
        serde_json::json!({
            "$wbEncrypted": 1,
            "envelope": b64().encode(serde_json::to_vec(&inner).unwrap()),
        })
    }

    #[test]
    fn round_trip_decrypts_to_original_token() {
        let token = "eyJhbGciOiJIUzI1NiJ9.payload.sig";
        let value = seal(SECRET, token);
        let envelope = parse_envelope(&value).expect("parse");
        assert_eq!(envelope.key_id, key_id_for(SECRET));
        assert_eq!(decrypt_envelope(&envelope, SECRET).unwrap(), token);
    }

    #[test]
    fn wrong_secret_is_key_mismatch() {
        let value = seal(SECRET, "token-abc");
        let envelope = parse_envelope(&value).unwrap();
        assert_eq!(
            decrypt_envelope(&envelope, "another-secret").unwrap_err(),
            TokenError::KeyMismatch
        );
    }

    #[test]
    fn tampered_ciphertext_fails_decrypt() {
        let mut value = seal(SECRET, "token-abc");
        let inner_b64 = value["envelope"].as_str().unwrap().to_string();
        let mut inner: Value = serde_json::from_slice(&b64().decode(&inner_b64).unwrap()).unwrap();
        // 翻转密文首字节：tag 校验必须失败。
        let mut ct = b64().decode(inner["ciphertext"].as_str().unwrap()).unwrap();
        ct[0] ^= 0xff;
        inner["ciphertext"] = serde_json::json!(b64().encode(ct));
        value["envelope"] = serde_json::json!(b64().encode(serde_json::to_vec(&inner).unwrap()));
        let envelope = parse_envelope(&value).unwrap();
        assert_eq!(
            decrypt_envelope(&envelope, SECRET).unwrap_err(),
            TokenError::DecryptFailed
        );
    }

    #[test]
    fn plaintext_token_is_not_an_envelope() {
        let err = parse_envelope(&serde_json::json!("eyJhbGciOi...")).unwrap_err();
        assert_eq!(err.code(), "INVALID_FORMAT");
    }

    #[test]
    fn unsupported_suite_is_rejected() {
        let mut value = seal(SECRET, "t");
        let inner_b64 = value["envelope"].as_str().unwrap().to_string();
        let mut inner: Value = serde_json::from_slice(&b64().decode(&inner_b64).unwrap()).unwrap();
        inner["suite"] = serde_json::json!(2);
        value["envelope"] = serde_json::json!(b64().encode(serde_json::to_vec(&inner).unwrap()));
        assert_eq!(
            parse_envelope(&value).unwrap_err(),
            TokenError::UnsupportedSuite(2)
        );
    }

    #[test]
    fn extra_or_missing_keys_are_rejected() {
        let mut value = seal(SECRET, "t");
        let inner_b64 = value["envelope"].as_str().unwrap().to_string();
        let mut inner: Value = serde_json::from_slice(&b64().decode(&inner_b64).unwrap()).unwrap();
        inner["extra"] = serde_json::json!(1);
        value["envelope"] = serde_json::json!(b64().encode(serde_json::to_vec(&inner).unwrap()));
        assert_eq!(parse_envelope(&value).unwrap_err().code(), "INVALID_FORMAT");
    }

    #[test]
    fn bad_key_id_shape_is_rejected() {
        let mut value = seal(SECRET, "t");
        let inner_b64 = value["envelope"].as_str().unwrap().to_string();
        let mut inner: Value = serde_json::from_slice(&b64().decode(&inner_b64).unwrap()).unwrap();
        inner["keyId"] = serde_json::json!("NOT-A-HEX-KEY");
        value["envelope"] = serde_json::json!(b64().encode(serde_json::to_vec(&inner).unwrap()));
        assert_eq!(parse_envelope(&value).unwrap_err().code(), "INVALID_FORMAT");
    }

    #[test]
    fn aad_layout_is_stable() {
        // 布局一旦改动，服务端就解不开：用 golden bytes 锁死。
        let aad = build_aad("0123456789abcdef");
        let mut expected = Vec::new();
        expected.extend_from_slice(b"WB-AAD\0");
        expected.push(0x01);
        expected.extend_from_slice(&[0, 0, 0, 5]);
        expected.extend_from_slice(b"WBEV1");
        expected.extend_from_slice(&[0, 0, 0, 6]);
        expected.extend_from_slice(b"sym-v1");
        expected.extend_from_slice(&[0, 0, 0, 1]);
        expected.extend_from_slice(&[0, 0, 0, 16]);
        expected.extend_from_slice(b"0123456789abcdef");
        expected.extend_from_slice(&[0x02, 0x00, 0x00]);
        assert_eq!(aad, expected);
        // 7 + 1 + (4+5) + (4+6) + 4 + (4+16) + 3
        assert_eq!(aad.len(), 54);
    }

    #[test]
    fn token_shape_guard() {
        assert!(looks_like_token("abc.def-ghi_jkl="));
        assert!(!looks_like_token(""));
        assert!(!looks_like_token("has space"));
        assert!(!looks_like_token(&"a".repeat(TOKEN_LIMIT + 1)));
    }
}
