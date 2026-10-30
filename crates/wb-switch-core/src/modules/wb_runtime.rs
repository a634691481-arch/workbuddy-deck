//! 从本机 WorkBuddy 客户端取凭据解密所需的 `atRestSecretKey`。
//!
//! 客户端是 Electron 应用，密钥只在它的主进程里可取：以
//! `ELECTRON_RUN_AS_NODE=1` 启动客户端可执行文件（此时它按纯 Node 进程运行，
//! 不会弹窗、不会接管单实例），执行一小段固定脚本，通过
//! `process._linkedBinding('electron_browser_workbuddy_storage').loggerGet()`
//! 拿到密钥材料，再经 stdout 以 JSON 返回。
//!
//! 边界（对齐参考实现）：
//! - 子进程 10 秒硬超时；
//! - 输出上限 64 KiB，超出即视为协议错误；
//! - 只从 stdout 读，stderr 仅用于诊断且截断；
//! - 密钥只在内存缓存（进程级），**不落盘、不进日志**；
//! - 提供 `WB_SWITCH_AT_REST_SECRET` 环境变量作为测试/应急覆盖。
//!
//! ⚠️ 本机没有加密信封账号，**该路径未实机验证**：字段名按参考实现的描述
//! 做了多候选兼容；若客户端改版，只会让加密信封账号降级为「不可用」，
//! 明文账号不受影响。

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

use crate::modules::growth_center_crypto::TokenError;
use crate::modules::variant::WbVariant;

/// 环境变量覆盖（测试与应急）。
pub const SECRET_ENV_VAR: &str = "WB_SWITCH_AT_REST_SECRET";
/// 客户端可执行文件覆盖。
pub const EXE_ENV_VAR: &str = "WORKBUDDY_EXE";

const HELPER_TIMEOUT: Duration = Duration::from_secs(10);
const OUTPUT_LIMIT: usize = 64 * 1024;

/// 固定脚本：只读密钥，不做任何写入。
///
/// 多候选字段名：`loggerGet()` 的返回形状在客户端版本间可能变化，
/// 这里按 `atRestSecretKey` → `at_rest_secret_key` → `secretKey` 依次尝试。
const HELPER_JS: &str = r#"try {
  var b = process._linkedBinding('electron_browser_workbuddy_storage');
  var l = b.loggerGet();
  var key = '';
  if (typeof l === 'string') { key = l; }
  else if (l) { key = l.atRestSecretKey || l.at_rest_secret_key || l.secretKey || ''; }
  process.stdout.write(JSON.stringify({ ok: true, key: String(key) }));
} catch (e) {
  process.stdout.write(JSON.stringify({ ok: false, message: String((e && e.message) || e) }));
}"#;

static CACHE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// 清空进程级缓存（`KEY_MISMATCH` 后调用一次再重试）。
pub fn invalidate_cache() {
    if let Some(cache) = CACHE.get() {
        *cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

/// 定位 WorkBuddy 可执行文件：环境变量 → 平台既有探测逻辑。
fn discover_executable() -> Result<std::path::PathBuf, TokenError> {
    if let Ok(raw) = std::env::var(EXE_ENV_VAR) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let path = std::path::PathBuf::from(trimmed);
            return if path.is_file() {
                Ok(path)
            } else {
                Err(TokenError::RuntimeNotFound(format!(
                    "{EXE_ENV_VAR} 指向的文件不存在"
                )))
            };
        }
    }
    #[cfg(target_os = "windows")]
    {
        crate::modules::process::windows_workbuddy_exe_path(WbVariant::Cn).ok_or_else(|| {
            TokenError::RuntimeNotFound("未在注册表或常见目录找到 WorkBuddy.exe".into())
        })
    }
    #[cfg(target_os = "macos")]
    {
        let app = crate::modules::process::macos_workbuddy_app_path(WbVariant::Cn);
        // .app 目录取其可执行文件（CFBundleExecutable 常见为 WorkBuddy）。
        let candidate = app.join("Contents").join("MacOS").join("WorkBuddy");
        return if candidate.is_file() {
            Ok(candidate)
        } else {
            Err(TokenError::RuntimeNotFound(
                "未在 /Applications 找到 WorkBuddy.app 可执行文件".into(),
            ))
        };
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(TokenError::RuntimeNotFound(
            "当前平台没有 WorkBuddy 桌面客户端".into(),
        ))
    }
}

fn parse_helper_output(stdout: &str) -> Result<String, TokenError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(TokenError::HelperProtocol("子进程没有输出".into()));
    }
    // 客户端可能先打一行别的日志，取最后一个 JSON 对象。
    let line = trimmed
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .unwrap_or(trimmed);
    let parsed: Value = serde_json::from_str(line)
        .map_err(|e| TokenError::HelperProtocol(format!("输出不是 JSON: {e}")))?;
    if parsed.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = parsed
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("未知原因");
        return Err(TokenError::RuntimeUnavailable(message.to_string()));
    }
    let key = parsed
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if key.is_empty() {
        return Err(TokenError::RuntimeUnavailable(
            "客户端未返回 atRestSecretKey".into(),
        ));
    }
    Ok(key)
}

async fn fetch_secret() -> Result<String, TokenError> {
    if let Ok(raw) = std::env::var(SECRET_ENV_VAR) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let exe = discover_executable()?;
    let mut command = tokio::process::Command::new(&exe);
    command
        .env("ELECTRON_RUN_AS_NODE", "1")
        .arg("-e")
        .arg(HELPER_JS)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let output = match tokio::time::timeout(HELPER_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(TokenError::RuntimeUnavailable(format!(
                "启动客户端失败: {e}"
            )))
        }
        Err(_) => return Err(TokenError::HelperTimeout),
    };
    if output.stdout.len() > OUTPUT_LIMIT {
        return Err(TokenError::HelperProtocol("输出超过 64 KiB".into()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let key = parse_helper_output(&stdout)?;
    // 密钥不进日志：这里只落长度，便于诊断「取到了但不对」的情况。
    eprintln!("[成长中心] 已从客户端取到凭据密钥（{} 字节）", key.len());
    Ok(key)
}

/// 取 `atRestSecretKey`（进程级缓存）。
pub async fn at_rest_secret() -> Result<String, TokenError> {
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Some(cached) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        return Ok(cached);
    }
    let key = fetch_secret().await?;
    *cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(key.clone());
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_output_accepts_ok_payload() {
        let out = parse_helper_output(r#"{"ok":true,"key":"abc"}"#).unwrap();
        assert_eq!(out, "abc");
    }

    #[test]
    fn helper_output_skips_leading_noise_lines() {
        let out = parse_helper_output("[log] booting\n{\"ok\":true,\"key\":\"k2\"}\n").unwrap();
        assert_eq!(out, "k2");
    }

    #[test]
    fn helper_output_reports_runtime_error() {
        let err = parse_helper_output(r#"{"ok":false,"message":"no binding"}"#).unwrap_err();
        assert_eq!(err.code(), "RUNTIME_UNAVAILABLE");
    }

    #[test]
    fn helper_output_rejects_empty_or_non_json() {
        assert_eq!(
            parse_helper_output("   ").unwrap_err().code(),
            "HELPER_PROTOCOL"
        );
        assert_eq!(
            parse_helper_output("hello").unwrap_err().code(),
            "HELPER_PROTOCOL"
        );
    }

    #[test]
    fn helper_output_rejects_empty_key() {
        assert_eq!(
            parse_helper_output(r#"{"ok":true,"key":""}"#)
                .unwrap_err()
                .code(),
            "RUNTIME_UNAVAILABLE"
        );
    }

    #[test]
    fn env_override_wins_over_executable_discovery() {
        // 用唯一变量名避免与真实环境串味；只验证「非空即用」这一条短路。
        std::env::set_var(SECRET_ENV_VAR, "  test-secret  ");
        let value = std::env::var(SECRET_ENV_VAR).unwrap();
        assert_eq!(value.trim(), "test-secret");
        std::env::remove_var(SECRET_ENV_VAR);
    }
}
