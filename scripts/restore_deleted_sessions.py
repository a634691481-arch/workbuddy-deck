"""从切换前备份恢复被「完全同步切换」删掉的会话正文。

背景：`replace` 模式会把目标账号的旧会话从 `~/.workbuddy/projects` 物理删除，
再写入源账号的会话。删除前的正文备份在
`~/.wb-switch/backups/switch-preflight-<UTC时间戳>/<sessionId>.jsonl`，
清单在同目录 `manifest.json`（含 sha256）。

删除时的 cwd / 标题记录在 `~/.wb-switch/session-replace-log.json`，按 ts 找到那一次操作，
就能把每个 sessionId 放回它原来的项目目录。

用法：
    python scripts/restore_deleted_sessions.py --ts 1795151233387            # 干跑，只打印
    python scripts/restore_deleted_sessions.py --ts 1795151233387 --apply    # 真写回
    ... --backup 20261120-050642                                             # 指定备份目录后缀

只做「补回缺失文件」：目标已存在就跳过，绝不覆盖现有会话。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys

WB_SWITCH = os.path.expanduser(r"~\.wb-switch")
PROJECTS = os.path.expanduser(r"~\.workbuddy\projects")


def encode_cwd(path: str) -> str:
    """WorkBuddy 的项目目录命名：盘符小写，`:` 与路径分隔符都换成 `-`，空格保留。"""
    unified = path.replace("/", "\\")
    drive, rest = unified.split(":", 1)
    return (drive.lower() + rest).replace("\\", "-")


def load_deleted(ts: int) -> dict[str, dict]:
    log_path = os.path.join(WB_SWITCH, "session-replace-log.json")
    with open(log_path, encoding="utf-8") as handle:
        records = json.load(handle)
    matches = [item for item in records if item.get("ts") == ts]
    if not matches:
        raise SystemExit(f"session-replace-log 里没有 ts={ts} 的记录")
    record = matches[0]
    print(
        f"操作 {ts}：{record['sourceUid']} → {record['targetUid']}，"
        f"删除 {len(record['deletedSessions'])} 条，复制 {record['copied']} 条"
    )
    return {item["id"]: item for item in record["deletedSessions"]}


def sha256_of(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ts", type=int, required=True, help="session-replace-log 里的操作时间戳(ms)")
    parser.add_argument("--backup", default="", help="备份目录名后缀，如 20261120-050642")
    parser.add_argument("--apply", action="store_true", help="真正写回（默认干跑）")
    args = parser.parse_args()

    deleted = load_deleted(args.ts)
    backups_root = os.path.join(WB_SWITCH, "backups")
    candidates = sorted(
        name
        for name in os.listdir(backups_root)
        if name.startswith("switch-preflight-") and args.backup in name
    )
    if not candidates:
        raise SystemExit(f"找不到备份目录：{backups_root}/switch-preflight-*{args.backup}")
    backup_dir = os.path.join(backups_root, candidates[-1])
    with open(os.path.join(backup_dir, "manifest.json"), encoding="utf-8") as handle:
        manifest = json.load(handle)
    print(f"备份目录：{backup_dir}（{len(manifest['files'])} 个文件）\n")

    planned = []
    for item in manifest["files"]:
        sid = item["sessionId"]
        meta = deleted.get(sid)
        if not meta:
            print(f"SKIP  {sid}  备份里没有对应的删除记录（{item.get('title')}）")
            continue
        target_dir = os.path.join(PROJECTS, encode_cwd(meta["cwd"]))
        target = os.path.join(target_dir, f"{sid}.jsonl")
        planned.append((item, target_dir, target))

    restored = skipped = 0
    for item, target_dir, target in planned:
        state = "存在" if os.path.exists(target) else "缺失"
        print(f"{state}  {item['title'][:28]:<30} {os.path.basename(target)}")
        if os.path.exists(target):
            skipped += 1
            continue
        if not args.apply:
            continue
        os.makedirs(target_dir, exist_ok=True)
        shutil.copy2(os.path.join(backup_dir, item["file"]), target)
        actual = sha256_of(target)
        if actual != item["sha256"]:
            raise SystemExit(f"校验失败：{target}（期望 {item['sha256']}，实际 {actual}）")
        restored += 1

    print(f"\n共 {len(planned)} 条：缺失 {len(planned) - skipped}，已存在 {skipped}")
    if args.apply:
        print(f"已写回 {restored} 条，sha256 全部校验通过。请重启 WorkBuddy 让侧栏重建索引。")
    else:
        print("干跑结束。加 --apply 才会真正写回。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
