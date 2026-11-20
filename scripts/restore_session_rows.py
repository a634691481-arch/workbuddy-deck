"""把恢复出来的会话正文重新挂回 WorkBuddy 索引库（`sessions` 表）。

为什么需要这一步：WorkBuddy 的侧栏读 `~/.workbuddy/workbuddy.db` 的 `sessions` 表，
只把 jsonl 文件放回 `projects/` 不够——完全同步切换时 DB 行也一起删了。

安全边界：
- 只 INSERT 缺失的 id，`INSERT OR IGNORE`，绝不 UPDATE/DELETE 现有行；
- 动手前先把 db 复制一份 `workbuddy.db.bak-restore-<ts>`；
- 必需字段按 DDL 的 NOT NULL / DEFAULT 填，其余留空（与同账号老行一致）。

用法：
    python scripts/restore_session_rows.py --ts 1795151233387            # 干跑
    python scripts/restore_session_rows.py --ts 1795151233387 --apply    # 写入
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sqlite3
import sys
import time

WB = os.path.expanduser(r"~\.workbuddy")
DB = os.path.join(WB, "workbuddy.db")


def load_deleted(ts: int) -> dict[str, dict]:
    with open(os.path.expanduser(r"~\.wb-switch\session-replace-log.json"), encoding="utf-8") as handle:
        records = json.load(handle)
    record = next((item for item in records if item.get("ts") == ts), None)
    if record is None:
        raise SystemExit(f"没有 ts={ts} 的替换记录")
    return {item["id"]: item for item in record["deletedSessions"]}, record


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ts", type=int, required=True)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()

    deleted, record = load_deleted(args.ts)
    target_uid = record["targetUid"]
    con = sqlite3.connect(DB)
    existing = {
        row[0]
        for row in con.execute(
            "select id from sessions where id in (%s)" % ",".join("?" * len(deleted)),
            tuple(deleted),
        )
    }
    missing = [sid for sid in deleted if sid not in existing]
    print(f"目标账号 {target_uid}：删除 {len(deleted)} 条，DB 中仍缺 {len(missing)} 条")
    for sid in missing:
        print(f"  {deleted[sid]['title'][:30]:<32} {sid}")
    if not missing:
        return 0
    if not args.apply:
        print("干跑结束；加 --apply 写入。")
        return 0

    backup = f"{DB}.bak-restore-{time.strftime('%Y%m%d-%H%M%S')}"
    shutil.copy2(DB, backup)
    print(f"已备份索引库：{backup}")

    rows = []
    for sid in missing:
        meta = deleted[sid]
        stamp = int(meta.get("updatedAt") or time.time() * 1000)
        rows.append(
            (
                sid,
                meta["cwd"],
                target_uid,
                meta.get("title"),
                "completed",
                stamp,
                stamp,
                stamp,
                1 if meta.get("isPlayground") else 0,
            )
        )
    with con:
        con.executemany(
            """
            insert or ignore into sessions
              (id, cwd, user_id, title, status, created_at, updated_at,
               last_activity_at, is_playground, transport, unread)
            values (?, ?, ?, ?, ?, ?, ?, ?, ?, 'local', 0)
            """,
            rows,
        )
    after = con.execute(
        "select count(*) from sessions where id in (%s)" % ",".join("?" * len(missing)),
        tuple(missing),
    ).fetchone()[0]
    print(f"写入完成：{after}/{len(missing)} 条已在索引中。重启 WorkBuddy 后侧栏可见。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
