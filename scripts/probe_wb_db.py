"""只读探测 WorkBuddy 本地索引库：确认被删会话是否还有 DB 记录。

用法：python scripts/probe_wb_db.py <db副本路径> [sessionId...]
"""

import sqlite3
import sys


def main() -> int:
    db = sys.argv[1]
    ids = sys.argv[2:]
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    tables = [row[0] for row in con.execute("select name from sqlite_master where type='table'")]
    print("tables:", tables)
    for table in tables:
        try:
            count = con.execute(f'select count(*) from "{table}"').fetchone()[0]
        except sqlite3.Error:
            continue
        print(f"{table}: {count}")
    for sid in ids:
        print(f"\n--- {sid} ---")
        for table in tables:
            cols = [row[1] for row in con.execute(f'pragma table_info("{table}")')]
            text_cols = [c for c in cols if "id" in c.lower() or "path" in c.lower() or "title" in c.lower()]
            for col in text_cols:
                try:
                    hit = con.execute(
                        f'select count(*) from "{table}" where "{col}" like ?', (f"%{sid}%",)
                    ).fetchone()[0]
                except sqlite3.Error:
                    continue
                if hit:
                    print(f"{table}.{col}: {hit}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
