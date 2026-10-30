//! 官方认证文件 `workbuddy-desktop.info` 的路径与读写（四段 JSON）。
//!
//! 对照 server.py `auth_file_path` / `workbuddy_app_path` / `read_auth_file` /
//! `import_from_auth_file`。切换写入（build_account_obj / build_auth_obj /
//! write_account_to_auth_file）在阶段 2 随 switch.rs 落地。

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

use crate::modules::account::{get_str, secret_value};
use crate::modules::config::{atomic_write, backup_dir, now_ms, utc_iso};
use crate::modules::variant::WbVariant;

/// WorkBuddy 官方认证文件路径（按档位，与 cockpit 一致）。
pub fn auth_file_path(variant: WbVariant) -> PathBuf {
    variant.auth_file_path()
}

/// WorkBuddy 应用路径（按档位）。
pub fn workbuddy_app_path(variant: WbVariant) -> PathBuf {
    #[cfg(target_os = "macos")]
    return crate::modules::process::macos_workbuddy_app_path(variant);

    #[cfg(target_os = "windows")]
    {
        // 探测顺序：运行进程 Path → 缓存 → 注册表 → 环境变量/盘符扫描。
        // 都找不到时返回 LOCALAPPDATA 默认路径，供启动失败文案写出尝试路径。
        if let Some(exe) = crate::modules::process::windows_workbuddy_exe_path(variant) {
            return exe;
        }
        crate::modules::process::windows_default_app_path(variant)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return variant.linux_app_path();
}

/// 读取认证文件 JSON；不存在或解析失败返回 None。
pub fn read_auth_file(variant: WbVariant) -> Option<Value> {
    read_auth_file_at(&auth_file_path(variant))
}

/// 读取指定路径的认证文件（单测注入临时登录态文件用）。
pub fn read_auth_file_at(path: &Path) -> Option<Value> {
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 切换前备份当前认证文件，返回备份路径。对照 server.py `backup_auth_file`。
pub fn backup_auth_file(variant: WbVariant) -> Option<PathBuf> {
    let path = auth_file_path(variant);
    if !path.exists() {
        return None;
    }
    let dir = backup_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let ts = utc_iso();
    let dest = dir.join(variant.backup_file_name(&ts));
    std::fs::copy(&path, &dest).ok()?;
    Some(dest)
}

/// 从账号库记录构造官方 account 字段。对照 server.py `build_account_obj`。
pub fn build_account_obj(acc: &Value) -> Value {
    let mut obj: Map<String, Value> = match acc.get("profile_raw") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    obj.insert(
        "uid".to_string(),
        acc.get("uid").cloned().unwrap_or_else(|| json!("")),
    );
    obj.insert(
        "nickname".to_string(),
        acc.get("nickname").cloned().unwrap_or_else(|| json!("")),
    );
    setdefault(&mut obj, "type", json!("personal"));
    setdefault(&mut obj, "accountType", json!(""));
    setdefault(&mut obj, "idp", json!(""));
    setdefault(&mut obj, "oneidAccountId", json!(""));
    setdefault(&mut obj, "areaInfoComplete", json!(false));
    setdefault(&mut obj, "isCurrentOneIdEnterprise", json!(false));
    setdefault(&mut obj, "isCurrentOneIdPersonal", json!(false));
    setdefault(&mut obj, "isFirstLogin", json!(false));
    setdefault(&mut obj, "isCreator", json!(false));
    setdefault(&mut obj, "isAdmin", json!(false));
    setdefault(&mut obj, "uin", json!(""));
    setdefault(&mut obj, "phoneNumber", json!(""));
    setdefault(&mut obj, "lastLogin", json!(true));
    setdefault(&mut obj, "pluginEnabled", json!(true));
    setdefault(
        &mut obj,
        "deployStatus",
        json!({"statusCode": 0, "statusMsg": "", "detailMsg": ""}),
    );
    setdefault(
        &mut obj,
        "sso",
        json!({"domain": "", "domainModifiedTimes": 0}),
    );
    Value::Object(obj)
}

/// 从账号库记录构造官方 auth 字段。对照 server.py `build_auth_obj`。
pub fn build_auth_obj(acc: &Value) -> Value {
    let mut obj: Map<String, Value> = Map::new();
    let raw = acc.get("auth_raw");
    if let Some(Value::Object(m)) = raw {
        let inner = match m.get("auth") {
            Some(Value::Object(im)) => im.clone(),
            _ => m.clone(),
        };
        obj.extend(inner);
    }
    let token_type = acc
        .get("token_type")
        .and_then(|v| v.as_str())
        .unwrap_or("Bearer")
        .to_string();
    let expires_at = acc.get("expiresAt").and_then(|v| v.as_i64());
    let now = now_ms();

    // token 可能是明文字符串或 WorkBuddy 5.6 加密信封：信封必须原样写回，
    // 由 WorkBuddy 读取时自行解密（同一 keyblob）。降级为空串会静默毁掉登录态。
    obj.insert(
        "accessToken".to_string(),
        secret_value(acc, "access_token").unwrap_or_else(|| json!("")),
    );
    obj.insert(
        "refreshToken".to_string(),
        secret_value(acc, "refresh_token").unwrap_or_else(|| json!("")),
    );
    obj.insert("tokenType".to_string(), token_type.into());
    obj.insert(
        "domain".to_string(),
        get_str(acc, "domain").unwrap_or_default().into(),
    );
    obj.insert("lastRefreshTime".to_string(), json!(now));
    setdefault(
        &mut obj,
        "scope",
        json!("openid profile offline_access email"),
    );

    if let Some(expires_at) = expires_at {
        obj.insert("expiresAt".to_string(), json!(expires_at));
        obj.insert(
            "expiresIn".to_string(),
            json!(((expires_at - now) / 1000).max(0)),
        );
        let refresh_exp = raw
            .and_then(|r| r.get("refreshExpiresAt"))
            .and_then(|v| v.as_i64())
            .unwrap_or(expires_at);
        if !obj.contains_key("refreshExpiresAt") {
            obj.insert("refreshExpiresAt".to_string(), json!(refresh_exp));
        }
        obj.insert(
            "refreshExpiresIn".to_string(),
            json!(((refresh_exp - now) / 1000).max(0)),
        );
    } else {
        setdefault(&mut obj, "expiresIn", json!(0));
        setdefault(&mut obj, "refreshExpiresIn", json!(0));
    }
    setdefault(&mut obj, "notBeforePolicy", json!(0));
    setdefault(&mut obj, "sessionState", json!(""));
    Value::Object(obj)
}

/// 把账号写入官方认证文件（原子写 + 写后校验）。对照 server.py `write_account_to_auth_file`。
pub fn write_account_to_auth_file(acc: &Value, variant: WbVariant) -> Result<(), String> {
    write_account_to_auth_file_at(acc, &auth_file_path(variant))
}

fn write_account_to_auth_file_at(acc: &Value, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let existing = read_auth_file_at(path).unwrap_or_else(|| json!({}));
    eprintln!(
        "[auth] write_account: existing is_object={} allAccounts_len={}",
        existing.is_object(),
        existing
            .get("allAccounts")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    );
    let all_accounts = existing
        .get("allAccounts")
        .cloned()
        .or_else(|| existing.get("accounts").cloned())
        .filter(|v| v.is_array())
        .unwrap_or_else(|| json!([]));
    let account_obj = build_account_obj(acc);
    let auth_obj = build_auth_obj(acc);

    // 把目标账号并入 allAccounts（去重：按 uid 或 id）
    let target_uid = get_str(acc, "uid").unwrap_or_default();
    let mut all: Vec<Value> = all_accounts.as_array().cloned().unwrap_or_default();
    all.retain(|a| {
        let primary = a
            .get("uid")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                a.get("id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("");
        primary != target_uid
    });
    all.push(account_obj.clone());
    eprintln!("[auth] write_account: merged allAccounts len={}", all.len());

    let session = json!({
        "account": &account_obj,
        "auth": &auth_obj,
        "accounts": &all,
        "allAccounts": &all,
    });
    let content = serde_json::to_string_pretty(&session).map_err(|e| e.to_string())?;
    if let Err(e) = atomic_write(path, &content) {
        eprintln!("[auth] atomic_write FAILED: {e}");
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            return Err(
                "无权限写入认证文件：请打开 系统设置→隐私与安全性→App 管理，允许本 App 控制 WorkBuddy 的数据（或为其开启『完全磁盘访问』后重试）"
                    .to_string(),
            );
        }
        return Err(e.to_string());
    }

    // 写后校验：按值比较（token 可能是明文字符串，也可能是 WorkBuddy 5.6 加密信封对象）
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let written_token = written
        .get("auth")
        .and_then(|a| a.get("accessToken"))
        .cloned()
        .unwrap_or(Value::Null);
    let expect_token = auth_obj.get("accessToken").cloned().unwrap_or(Value::Null);
    if written_token != expect_token {
        return Err("认证文件写后校验失败，未写入目标账号".to_string());
    }

    // 写后校验 2：身份一致（uid 非空时）。token 对但 uid 不对 = 并发回写或写到别处，
    // 必须在写入层就失败，不能等到上层才发现「切过去了但切错了人」。
    // uid 为空或为加密信封（取不到明文字符串）时跳过，由上层标 identity-unverified。
    let expected_uid = get_str(acc, "uid").unwrap_or_default();
    if !expected_uid.is_empty() {
        let written_uid = written
            .get("account")
            .and_then(|account| get_str(account, "uid"))
            .unwrap_or_default();
        if written_uid != expected_uid {
            return Err(format!(
                "认证文件写后身份校验失败：期望 uid {expected_uid}，实际 {written_uid}"
            ));
        }
    }

    // WorkBuddy 优先读取退出标记：即使认证文件已有凭据，标记仍存在时也视为未登录。
    // 必须在写入及校验成功后清理；失败时保留退出状态，并阻止调用方报告切换成功。
    let marker = logout_marker_path(path);
    match std::fs::remove_file(&marker) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "清理 WorkBuddy 退出标记失败（{}）：{error}",
                marker.display()
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 写后身份校验与事务回滚
// ---------------------------------------------------------------------------

/// 写后身份校验结果（三态，对齐 modeldeck 的 effective / identity-mismatch / identity-unverified）。
///
/// `Mismatch` 是**失败**：文件确实写成功了，但磁盘上的身份不是目标账号（并发回写、
/// 写到别的档位、被客户端覆盖）。此时必须回滚，否则用户以为切过去了，实际在别人的号上。
/// `Unverified` 是**警告**：账号本身没有可判定身份的字段，无法校验，不阻断但要如实标注。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityState {
    Effective,
    Mismatch,
    Unverified,
}

impl IdentityState {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentityState::Effective => "effective",
            IdentityState::Mismatch => "identity-mismatch",
            IdentityState::Unverified => "identity-unverified",
        }
    }

    /// 只有明确不匹配才阻断；无法校验不阻断（否则无 uid 的老账号直接不可切换）。
    pub fn blocks_switch(self) -> bool {
        matches!(self, IdentityState::Mismatch)
    }
}

/// 读取认证文件在磁盘上声明的身份：`account.uid` → `account.id` → 顶层 uid。
///
/// 只读磁盘，不读内存：校验的就是用户开 App 后真正会读到的那份。
///
/// 顺序与写入层（`write_account_to_auth_file` 校验 `account.uid`）保持一致的**账号对象优先**：
/// 顶层 `uid` 只是兜底。若反过来先取顶层，一旦 WorkBuddy 日后在文件根部写入设备级
/// `uid`，每次切换都会被判成 identity-mismatch 并回滚——把成功的切换作废。
pub fn auth_identity_at(path: &Path) -> Option<String> {
    let root = read_auth_file_at(path)?;
    let account_obj = root.get("account").filter(|v| v.is_object()).cloned();
    let picked = account_obj
        .as_ref()
        .and_then(|a| get_str(a, "uid"))
        .or_else(|| account_obj.as_ref().and_then(|a| get_str(a, "id")))
        .or_else(|| get_str(&root, "uid"));
    picked.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// 校验磁盘认证文件的身份是否为 `expected_uid`。
///
/// `expected_uid` 为空或磁盘读不回身份 → `Unverified`；不一致 → `Mismatch`。
pub fn verify_identity_at(path: &Path, expected_uid: &str) -> IdentityState {
    let expected = expected_uid.trim();
    if expected.is_empty() {
        return IdentityState::Unverified;
    }
    match auth_identity_at(path) {
        Some(actual) if actual == expected => IdentityState::Effective,
        Some(_) => IdentityState::Mismatch,
        None => IdentityState::Unverified,
    }
}

/// 按档位校验（写后调用）。
pub fn verify_identity(variant: WbVariant, expected_uid: &str) -> IdentityState {
    verify_identity_at(&auth_file_path(variant), expected_uid)
}

/// 切换前的认证文件原始字节快照。`None` 表示文件当时不存在。
///
/// 用原始字节而非解析后的 JSON：回滚时不能因为序列化丢字段、改键序、动精度而
/// 让「还原」变成「另一个版本」。
pub fn snapshot_auth_file(variant: WbVariant) -> Option<Vec<u8>> {
    std::fs::read(auth_file_path(variant)).ok()
}

/// 用快照还原认证文件：`Some` 写回原字节，`None` 删除文件（原本就没有）。
pub fn restore_auth_file(variant: WbVariant, snapshot: Option<&[u8]>) -> Result<(), String> {
    restore_auth_file_at(&auth_file_path(variant), snapshot)
}

fn restore_auth_file_at(path: &Path, snapshot: Option<&[u8]>) -> Result<(), String> {
    let Some(bytes) = snapshot else {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("还原认证文件失败（删除）：{error}")),
        };
    };
    atomic_write_bytes(path, bytes).map_err(|error| format!("还原认证文件失败：{error}"))
}

/// 字节级原子写（tmp + rename），与 `config::atomic_write` 同语义但保留原始编码。
fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(
        "{file_name}.tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&tmp, bytes)?;
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

/// 官方退出标记是在完整认证文件名后追加后缀，不能替换 `.info` 扩展名。
fn logout_marker_path(auth_path: &Path) -> PathBuf {
    let mut marker = auth_path.as_os_str().to_os_string();
    marker.push(".logged-out");
    PathBuf::from(marker)
}

fn setdefault(map: &mut Map<String, Value>, key: &str, value: Value) {
    if !map.contains_key(key) {
        map.insert(key.to_string(), value);
    }
}

/// 从当前 WorkBuddy 登录态导入账号。对照 server.py `import_from_auth_file`。
pub fn import_from_auth_file(variant: WbVariant) -> Option<Value> {
    imported_account_from_root(read_auth_file(variant)?, variant)
}

fn imported_account_from_root(root: Value, variant: WbVariant) -> Option<Value> {
    let account_obj = root
        .get("account")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let auth_obj = root
        .get("auth")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));

    let uid = get_str(&root, "uid").or_else(|| get_str(&account_obj, "uid"));
    let uid = uid.or_else(|| get_str(&account_obj, "id"));
    // WorkBuddy 5.6 起 nickname/accessToken/refreshToken 可能是 `{$wbEncrypted, envelope}`
    // 加密信封：这里必须原样保留（secret_value），不能 get_str 强转字符串——
    // 否则 accessToken 取不到导致导入恒 400，切换写回时也会把信封覆盖成空串。
    let nickname = secret_value(&root, "nickname")
        .or_else(|| secret_value(&root, "name"))
        .or_else(|| secret_value(&account_obj, "nickname"))
        .or_else(|| secret_value(&account_obj, "label"));
    let email = get_str(&root, "email")
        .or_else(|| get_str(&account_obj, "email"))
        .or_else(|| get_str(&auth_obj, "email"));
    let access_token = secret_value(&auth_obj, "accessToken")
        .or_else(|| secret_value(&auth_obj, "access_token"))
        .or_else(|| secret_value(&root, "accessToken"))
        .or_else(|| secret_value(&root, "access_token"));
    let refresh_token = secret_value(&auth_obj, "refreshToken")
        .or_else(|| secret_value(&auth_obj, "refresh_token"))
        .or_else(|| secret_value(&root, "refreshToken"))
        .or_else(|| secret_value(&root, "refresh_token"));
    let token_type = get_str(&auth_obj, "tokenType")
        .or_else(|| get_str(&auth_obj, "token_type"))
        .unwrap_or_else(|| "Bearer".to_string());
    let domain = get_str(&root, "domain").or_else(|| get_str(&auth_obj, "domain"));
    let expires_at = parse_ts(root.get("expiresAt").or_else(|| auth_obj.get("expiresAt")));
    let refresh_expires_at = parse_ts(
        root.get("refreshExpiresAt")
            .or_else(|| auth_obj.get("refreshExpiresAt")),
    );

    access_token.as_ref()?;

    Some(json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "uid": uid,
        "nickname": nickname,
        "email": email,
        "variant": variant.as_str(),
        "enterpriseName": get_str(&root, "enterpriseName")
            .or_else(|| get_str(&root, "enterprise_name"))
            .or_else(|| get_str(&account_obj, "enterpriseName"))
            .or_else(|| get_str(&account_obj, "enterprise_name")),
        "enterpriseId": get_str(&root, "enterpriseId")
            .or_else(|| get_str(&root, "enterprise_id"))
            .or_else(|| get_str(&account_obj, "enterpriseId"))
            .or_else(|| get_str(&account_obj, "enterprise_id")),
        "access_token": access_token,
        "refresh_token": refresh_token,
        "token_type": token_type,
        "domain": domain,
        "expiresAt": expires_at,
        "refreshExpiresAt": refresh_expires_at,
        "auth_raw": root,
        "profile_raw": account_obj,
        "createdAt": now_ms(),
    }))
}

/// 字符串时间戳转 i64（数字原样保留，不做秒/毫秒换算）。
/// 对照 server.py `import_from_auth_file` 的 str→int 逻辑。
fn parse_ts(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TempAuthDir(PathBuf);

    impl TempAuthDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "wb-switch-auth-file-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn auth_path(&self, variant: WbVariant) -> PathBuf {
            self.0.join(auth_file_path(variant).file_name().unwrap())
        }

        fn marker_path(&self, variant: WbVariant) -> PathBuf {
            // 独立指定官方文件名，避免测试与生产实现共享错误的后缀拼接规则。
            self.0.join(match variant {
                WbVariant::Cn => "workbuddy-desktop.info.logged-out",
                WbVariant::Ai => "workbuddy-desktop-ai.info.logged-out",
            })
        }
    }

    impl Drop for TempAuthDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn successful_auth_write_clears_only_target_logout_marker() {
        for variant in [WbVariant::Cn, WbVariant::Ai] {
            for token in [
                json!("test-access-token"),
                json!({"$wbEncrypted": 1, "envelope": {"v": 1, "wrapped": "test"}}),
            ] {
                let dir = TempAuthDir::new();
                let path = dir.auth_path(variant);
                let marker = dir.marker_path(variant);
                let other_variant = match variant {
                    WbVariant::Cn => WbVariant::Ai,
                    WbVariant::Ai => WbVariant::Cn,
                };
                let other_path = dir.auth_path(other_variant);
                let other_marker = dir.marker_path(other_variant);
                std::fs::write(&marker, "logged-out").unwrap();
                std::fs::write(&other_marker, "other-logout").unwrap();
                std::fs::write(&other_path, "other-session").unwrap();
                let account = json!({"uid": "target", "access_token": token});

                write_account_to_auth_file_at(&account, &path).unwrap();

                assert!(
                    !marker.exists(),
                    "WorkBuddy 会忽略带退出标记的认证文件，切换成功必须清理标记"
                );
                let written = read_auth_file_at(&path).unwrap();
                assert_eq!(written["auth"]["accessToken"], token);
                assert_eq!(written["account"]["uid"], "target");
                assert_eq!(
                    std::fs::read_to_string(other_marker).unwrap(),
                    "other-logout"
                );
                assert_eq!(
                    std::fs::read_to_string(other_path).unwrap(),
                    "other-session"
                );
            }
        }
    }

    #[test]
    fn auth_write_without_logout_marker_remains_idempotent() {
        let dir = TempAuthDir::new();
        let path = dir.auth_path(WbVariant::Cn);
        let account = json!({"uid": "target", "access_token": "test-access-token"});

        write_account_to_auth_file_at(&account, &path).unwrap();
        write_account_to_auth_file_at(&account, &path).unwrap();

        assert!(!dir.marker_path(WbVariant::Cn).exists());
        let written = read_auth_file_at(&path).unwrap();
        assert_eq!(written["auth"]["accessToken"], "test-access-token");
        assert_eq!(written["allAccounts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn failed_auth_write_preserves_logout_marker() {
        let dir = TempAuthDir::new();
        let path = dir.auth_path(WbVariant::Cn);
        let marker = dir.marker_path(WbVariant::Cn);
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), "existing-data").unwrap();
        std::fs::write(&marker, "logged-out").unwrap();
        let account = json!({"uid": "target", "access_token": "test-access-token"});

        assert!(write_account_to_auth_file_at(&account, &path).is_err());

        assert_eq!(std::fs::read_to_string(marker).unwrap(), "logged-out");
        assert_eq!(
            std::fs::read_to_string(path.join("keep")).unwrap(),
            "existing-data"
        );
    }

    #[test]
    fn failed_logout_marker_removal_is_not_reported_as_success() {
        let dir = TempAuthDir::new();
        let path = dir.auth_path(WbVariant::Cn);
        let marker = dir.marker_path(WbVariant::Cn);
        // 用同名非空目录模拟无法 remove_file 的标记，不依赖平台权限行为。
        std::fs::create_dir(&marker).unwrap();
        std::fs::write(marker.join("keep"), "existing-data").unwrap();
        let account = json!({"uid": "target", "access_token": "test-access-token"});

        let error = write_account_to_auth_file_at(&account, &path).unwrap_err();

        assert!(
            error.contains("退出标记"),
            "错误应说明未清理退出标记：{error}"
        );
        assert!(marker.exists());
        assert_eq!(
            read_auth_file_at(&path).unwrap()["account"]["uid"],
            "target"
        );
    }

    #[test]
    fn auth_file_path_is_expected_location() {
        let p = auth_file_path(WbVariant::Cn);
        let s = p.to_string_lossy();
        assert!(
            s.contains("CodeBuddyExtension"),
            "路径应包含 CodeBuddyExtension: {s}"
        );
        assert!(
            s.ends_with("workbuddy-desktop.info"),
            "文件名应为 workbuddy-desktop.info: {s}"
        );

        // 国际版：同目录、不同文件
        let ai = auth_file_path(WbVariant::Ai);
        assert_eq!(ai.parent(), p.parent());
        assert!(ai.to_string_lossy().ends_with("workbuddy-desktop-ai.info"));
    }

    #[test]
    fn backup_file_name_is_distinguishable_per_variant() {
        let ts = "2026-09-15T00-00-00Z";
        assert_eq!(
            WbVariant::Cn.backup_file_name(ts),
            "workbuddy-desktop.2026-09-15T00-00-00Z.info"
        );
        assert_eq!(
            WbVariant::Ai.backup_file_name(ts),
            "workbuddy-desktop-ai.2026-09-15T00-00-00Z.info"
        );
    }

    #[test]
    fn import_from_auth_file_extracts_fields() {
        let root = json!({
            "account": {"uid": "u-1", "nickname": "小明", "email": "a@b.c"},
            "auth": {
                "accessToken": "AT-1",
                "refreshToken": "RT-1",
                "tokenType": "Bearer",
                "domain": "www.codebuddy.cn",
                "expiresAt": "1791912333558",
            },
            "domain": "www.codebuddy.cn",
        });
        // import_from_auth_file 从真实认证文件读取，此处直接测 parse_ts 与字段提取逻辑
        assert_eq!(parse_ts(root["auth"].get("expiresAt")), Some(1791912333558));
        assert_eq!(parse_ts(root["auth"].get("refreshToken")), None);
        assert_eq!(parse_ts(Some(&json!("1786728333"))), Some(1786728333));
    }

    #[test]
    fn import_without_email_does_not_synthesize_one() {
        let account = imported_account_from_root(
            json!({
                "account": {"uid": "u-1", "nickname": "同名用户"},
                "auth": {"accessToken": "test-token"}
            }),
            WbVariant::Cn,
        )
        .expect("auth payload should import");

        assert_eq!(account["uid"], "u-1");
        assert_eq!(account["nickname"], "同名用户");
        assert!(account["email"].is_null());
        assert_eq!(account["variant"], "cn");
    }

    /// 回归：`"accessToken": ""`（含纯空白）的登录态必须判为「没有 token」。
    /// 修复前 `secret_value` 对空串返回 `Some("")`，`imported_account_from_root` 里
    /// `access_token.is_none()` 的检查拦不住，导入会落库一条空凭据账号；
    /// `/api/import-local` 也因此不再返回 400「未读取到本地 WorkBuddy 登录信息」。
    #[test]
    fn blank_access_token_is_not_imported() {
        for blank in ["", "   ", "\t\n"] {
            let imported = imported_account_from_root(
                json!({
                    "account": {"uid": "u-1", "nickname": "小明"},
                    "auth": {"accessToken": blank, "refreshToken": "RT-1"}
                }),
                WbVariant::Cn,
            );
            assert!(imported.is_none(), "空 accessToken（{blank:?}）不得被导入");
        }

        // 加密信封不受影响：仍原样保留，切换写回依赖它。
        let envelope = json!({"$wbEncrypted": 1, "envelope": "enc"});
        let imported = imported_account_from_root(
            json!({
                "account": {"uid": "u-1"},
                "auth": {"accessToken": envelope.clone()}
            }),
            WbVariant::Cn,
        )
        .expect("加密信封 accessToken 必须可导入");
        assert_eq!(imported["access_token"], envelope);
    }

    #[test]
    fn imported_account_records_source_variant() {
        let account = imported_account_from_root(
            json!({
                "account": {"uid": "u-ai"},
                "auth": {"accessToken": "at-ai", "domain": "www.workbuddy.ai"}
            }),
            WbVariant::Ai,
        )
        .expect("auth payload should import");

        assert_eq!(account["variant"], "ai");
        assert_eq!(
            crate::modules::account::variant_of(&account),
            WbVariant::Ai,
            "导入账号自带档位，无需依赖域名兜底"
        );
    }

    /// 写入一份最小可判身份认证文件。
    fn write_auth(path: &Path, uid: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            path,
            json!({
                "account": {"uid": uid, "nickname": uid},
                "auth": {"accessToken": "token"},
            })
            .to_string(),
        )
        .unwrap();
    }

    /// 三态判定：一致 Effective、不一致 Mismatch、无判据 Unverified。
    #[test]
    fn identity_state_detects_match_mismatch_and_unknown() {
        let dir = TempAuthDir::new();
        let path = dir.0.join("auth.info");
        write_auth(&path, "uid-a");

        assert_eq!(
            verify_identity_at(&path, "uid-a"),
            IdentityState::Effective
        );
        assert_eq!(
            verify_identity_at(&path, "uid-b"),
            IdentityState::Mismatch,
            "文件里的身份不是期望账号时必须报 Mismatch"
        );
        // 空白/缺失判据不当作成功，也不当作失败
        assert_eq!(verify_identity_at(&path, ""), IdentityState::Unverified);
        assert_eq!(verify_identity_at(&path, "   "), IdentityState::Unverified);
        // 读不回来（文件不存在）
        assert_eq!(
            verify_identity_at(&dir.0.join("missing.info"), "uid-a"),
            IdentityState::Unverified
        );
        assert!(IdentityState::Mismatch.blocks_switch());
        assert!(!IdentityState::Unverified.blocks_switch());
        let _ = std::fs::remove_dir_all(&dir.0);
    }

    /// 身份字段缺失时退回 account.id；两者都没有才是 Unverified。
    #[test]
    fn identity_falls_back_to_account_id() {
        let dir = TempAuthDir::new();
        let path = dir.0.join("auth-id.info");
        std::fs::write(&path, json!({"account": {"id": "acc-9"}}).to_string()).unwrap();
        assert_eq!(auth_identity_at(&path).as_deref(), Some("acc-9"));
        assert_eq!(verify_identity_at(&path, "acc-9"), IdentityState::Effective);

        let blank = dir.0.join("auth-blank.info");
        std::fs::write(&blank, json!({"account": {"uid": "  "}}).to_string()).unwrap();
        assert_eq!(auth_identity_at(&blank), None);
        assert_eq!(
            verify_identity_at(&blank, "uid-a"),
            IdentityState::Unverified
        );
        let _ = std::fs::remove_dir_all(&dir.0);
    }

    /// 快照 → 被改坏 → 还原：必须**逐字节**回到切换前，而不是「等价 JSON」。
    #[test]
    fn snapshot_restore_is_byte_exact() {
        let dir = TempAuthDir::new();
        let path = dir.0.join("auth-restore.info");
        // 刻意用非规范格式（键序、缩进、尾随空格），验证还原不重新序列化。
        let original = b"{\n  \"auth\" :{\"accessToken\":\"a\"},\n  \"account\": {\"uid\":\"uid-a\"}\n}\n  ";
        std::fs::write(&path, original).unwrap();
        let snapshot = std::fs::read(&path).ok();
        assert!(snapshot.is_some());

        // 模拟写后校验失败：文件已被另一个身份覆盖
        write_auth(&path, "uid-b");
        assert_eq!(verify_identity_at(&path, "uid-a"), IdentityState::Mismatch);

        restore_auth_file_at(&path, snapshot.as_deref()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(verify_identity_at(&path, "uid-a"), IdentityState::Effective);
        let _ = std::fs::remove_dir_all(&dir.0);
    }

    /// 切换前文件不存在（首次登录）→ 还原意味着删除，而不是留下一个空文件。
    #[test]
    fn restore_without_snapshot_removes_file() {
        let dir = TempAuthDir::new();
        let path = dir.0.join("auth-none.info");
        write_auth(&path, "uid-b");
        restore_auth_file_at(&path, None).unwrap();
        assert!(!path.exists(), "原本不存在的认证文件，还原后也不能存在");
        // 幂等：再还原一次不报错
        restore_auth_file_at(&path, None).unwrap();
        let _ = std::fs::remove_dir_all(&dir.0);
    }
}
