"""把「完全同步」误删的会话从关联组副本恢复回原账号。

背景：切换 A→B 时 core 把 A 的会话复制成 B 的新会话（原 id 保留在关联组里记录），
再切回 B→A 时 `resolve_target` 命中关联组里 A 的旧成员 → 判定 alreadyLinked 不复制，
随后 replace 仍按「复制成功」执行物理删除 → A 的会话被清空。

恢复依据：
- `~/.wb-switch/session_links.json`：groups[].members 里同一逻辑会话在原账号(id_orig)
  与目标账号(id_copy) 各有一条 active 成员；
- `~/.workbuddy/projects/**/<id_copy>.jsonl`：正文仍在（复制品）；
- `~/.workbuddy/workbuddy.db`：副本行仍在。

做法：正文按 core 同一规则重写（文本替换 id）后落到原目录的原 id 文件名；
再从副本行复制一条 sessions 行，id 换回原 id、user_id 换回原账号。

用法：
    python tasks/restore_sessions.py            # 预演，只统计
    python tasks/restore_sessions.py --apply    # 真实写入
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sqlite3
import time
from pathlib import Path

HOME = Path(os.path.expanduser("~"))
DB = HOME / ".workbuddy" / "workbuddy.db"
PROJECTS = HOME / ".workbuddy" / "projects"
LINKS = HOME / ".wb-switch" / "session_links.json"

# 本机两个账号：被误删的一方（原账号 181****6649）与提供副本的一方（xiz🍉）
TARGET_UID = "c88ff3a8-4277-4a05-a31b-b161d942743b"
SOURCE_UID = "c4ddd071-3cec-4d06-8a9e-9e66391bcdd5"


def body_index() -> dict[str, Path]:
    index: dict[str, Path] = {}
    for root, _dirs, files in os.walk(PROJECTS):
        for name in files:
            if name.endswith(".jsonl"):
                index[name[:-6]] = Path(root) / name
    return index


def link_pairs() -> list[tuple[str, str]]:
    """返回 [(原账号 sessionId, 副本 sessionId)]。"""
    store = json.loads(LINKS.read_text(encoding="utf-8"))
    pairs: list[tuple[str, str]] = []
    for group in store.get("groups", []):
        members = {
            member["uid"]: member["sessionId"]
            for member in group.get("members", [])
            if member.get("state") == "active"
        }
        if TARGET_UID in members and SOURCE_UID in members:
            pairs.append((members[TARGET_UID], members[SOURCE_UID]))
    return pairs


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--apply", action="store_true", help="真实写入（默认只预演）")
    args = parser.parse_args()

    index = body_index()
    pairs = link_pairs()
    conn = sqlite3.connect(DB)
    conn.execute("PRAGMA busy_timeout = 15000")

    cur = conn.cursor()
    cur.execute("SELECT * FROM sessions LIMIT 0")
    columns = [col[0] for col in cur.description]

    plan: list[tuple[str, str, Path]] = []
    skipped: list[tuple[str, str]] = []
    for orig_id, copy_id in pairs:
        if copy_id not in index:
            skipped.append((orig_id, "副本正文缺失"))
            continue
        plan.append((orig_id, copy_id, index[copy_id]))
    print(f"关联组: {len(pairs)} 对；可恢复: {len(plan)}；跳过: {len(skipped)}")
    for orig_id, reason in skipped:
        print(f"  skip {orig_id[:8]} {reason}")
    if not args.apply:
        print("预演结束（加 --apply 才写入）")
        return 0

    backup_dir = Path(__file__).with_name("restore-backup") / time.strftime("%Y%m%d-%H%M%S")
    backup_dir.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(f"file:{DB}?mode=ro", uri=True) as src, sqlite3.connect(backup_dir / "workbuddy.db") as dst:
        src.backup(dst)
    print(f"数据库快照: {backup_dir / 'workbuddy.db'}")

    stamp = int(time.time() * 1000)
    restored = 0
    for orig_id, copy_id, copy_path in plan:
        source_rows = cur.execute(
            f"SELECT {', '.join(columns)} FROM sessions WHERE id = ?", (copy_id,)
        ).fetchall()
        if not source_rows:
            skipped.append((orig_id, "副本行缺失"))
            continue
        source = dict(zip(columns, source_rows[0]))

        # 1) 正文：与 core 同规则，整段文本替换 id 后落到副本所在工作区目录。
        dest = copy_path.with_name(f"{orig_id}.jsonl")
        if not dest.exists():
            text = copy_path.read_text(encoding="utf-8", errors="replace").replace(copy_id, orig_id)
            tmp = dest.with_suffix(".jsonl.tmp")
            tmp.write_text(text, encoding="utf-8")
            shutil.move(str(tmp), str(dest))

        # 2) 会话行：副本行改回原 id / 原账号；时间戳保留副本行的采集时刻（内容真实时间不变）。
        row = {key: source[key] for key in columns}
        row["id"] = orig_id
        row["user_id"] = TARGET_UID
        row["deleted_at"] = None
        if row.get("updated_at") in (None, 0):
            row["updated_at"] = stamp
        existed = cur.execute(
            "SELECT 1 FROM sessions WHERE id = ? AND user_id = ?", (orig_id, TARGET_UID)
        ).fetchone()
        if existed:
            assignments = ", ".join(f"{key} = ?" for key in columns if key not in ("id", "user_id"))
            values = [row[key] for key in columns if key not in ("id", "user_id")]
            cur.execute(
                f"UPDATE sessions SET {assignments} WHERE id = ? AND user_id = ?",
                (*values, orig_id, TARGET_UID),
            )
        else:
            placeholders = ", ".join("?" for _ in columns)
            cur.execute(
                f"INSERT INTO sessions ({', '.join(columns)}) VALUES ({placeholders})",
                [row[key] for key in columns],
            )
        restored += 1

    conn.commit()

    visible = conn.execute(
        "SELECT count(*) FROM sessions WHERE user_id = ? AND deleted_at IS NULL", (TARGET_UID,)
    ).fetchone()[0]
    print(f"已恢复 {restored} 条；原账号当前可见会话 {visible} 条")
    conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
