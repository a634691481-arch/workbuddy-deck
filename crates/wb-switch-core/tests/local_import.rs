//! 回归：没装过 workbuddy-switch 的机器，也要能从 WorkBuddy 自己的登录态建出账号。
//!
//! 现场：用户机器上不存在 `~/.wb-switch/accounts.json`，但 WorkBuddy 已登录
//! （登录态在 `.../CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info`）。
//! 旧行为：任务面板与账号管理都是空的 —— 因为没有任何代码去读那个 auth 文件。
//!
//! 这里用 `WB_SWITCH_HOME` 指向临时目录，构造一个「干净的机器」：只有 auth 文件、
//! 没有账号库，验证 `account::import_local` 能把账号落库；再调一次验证幂等（不重复）。

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::json;
use wb_switch_core::modules::account;
use wb_switch_core::modules::auth_file;
use wb_switch_core::modules::config;
use wb_switch_core::modules::variant::WbVariant;

/// `WB_SWITCH_HOME` 是进程级环境变量：两个用例必须串行，否则互相改路径。
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn temp_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wb-import-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp home");
    dir
}

/// 写一份「已登录」的 WorkBuddy 登录态文件（字段形状取自真实文件，token 用假值）。
fn write_auth_file(home: &PathBuf, variant: WbVariant, uid: &str, nickname: &str) {
    let _ = home;
    let path = auth_file::auth_file_path(variant);
    fs::create_dir_all(path.parent().unwrap()).expect("mkdir auth dir");
    let payload = json!({
        "account": { "uid": uid, "nickname": nickname, "type": "personal" },
        "auth": {
            "accessToken": format!("token-{uid}"),
            "refreshToken": format!("refresh-{uid}"),
            "tokenType": "Bearer",
            "expiresAt": config::now_ms() + 3_600_000_i64
        }
    });
    fs::write(&path, serde_json::to_vec(&payload).unwrap()).expect("write auth file");
}

#[test]
fn local_import_builds_account_store_from_workbuddy_login() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = temp_home("fresh");
    std::env::set_var(config::HOME_ENV_VAR, &home);

    // 干净的机器：账号库不存在
    assert!(!config::accounts_file().exists(), "precondition: no store");

    write_auth_file(&home, WbVariant::Cn, "uid-fresh", "本机账号");

    let meta = account::import_local(WbVariant::Cn).expect("import from local login");
    assert_eq!(meta.get("uid").and_then(|v| v.as_str()), Some("uid-fresh"));

    let saved = account::load_accounts();
    assert_eq!(saved.len(), 1, "import must create exactly one account");
    assert_eq!(saved[0].get("uid").and_then(|v| v.as_str()), Some("uid-fresh"));
    assert_eq!(
        saved[0].get("access_token").and_then(|v| v.as_str()),
        Some("token-uid-fresh")
    );
    assert!(saved[0].get("id").and_then(|v| v.as_str()).is_some(), "store requires id");

    // 幂等：再次导入同一账号不得产生重复行（挂载时静默导入会反复跑）
    account::import_local(WbVariant::Cn).expect("second import");
    assert_eq!(account::load_accounts().len(), 1, "import must be idempotent");

    // 没有登录态的档位给出可读错误，而不是静默成功
    assert!(account::import_local(WbVariant::Ai).is_err());

    let _ = fs::remove_dir_all(&home);
}


/// 兼容「装过 workbuddy-switch」的机器：账号库已存在同一 uid 时必须空转，
/// 绝不能拿 auth 文件里的旧 token 覆盖库里刷新过的新 token。
#[test]
fn local_import_if_absent_keeps_refreshed_token_of_existing_account() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = temp_home("existing");
    std::env::set_var(config::HOME_ENV_VAR, &home);

    // 账号库已有该账号，且 token 是「刷新过的」新值
    let store_dir = config::store_dir();
    fs::create_dir_all(&store_dir).unwrap();
    fs::write(
        config::accounts_file(),
        serde_json::to_vec(&json!([{
            "id": "local-id",
            "uid": "uid-exists",
            "nickname": "库里账号",
            "access_token": "refreshed-token",
            "refresh_token": "refreshed-refresh",
            "expiresAt": config::now_ms() + 3_600_000_i64
        }]))
        .unwrap(),
    )
    .unwrap();

    // auth 文件里是同一 uid 的旧 token
    write_auth_file(&home, WbVariant::Cn, "uid-exists", "auth 昵称");

    let imported = account::import_local_if_absent(WbVariant::Cn).expect("scan");
    assert!(imported.is_none(), "existing uid must be skipped");

    let saved = account::load_accounts();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0].get("access_token").and_then(|v| v.as_str()),
        Some("refreshed-token"),
        "import must not overwrite a refreshed token with the auth file copy"
    );
    assert_eq!(saved[0].get("nickname").and_then(|v| v.as_str()), Some("库里账号"));

    let _ = fs::remove_dir_all(&home);
}
