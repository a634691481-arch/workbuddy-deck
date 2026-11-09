"""生成本次发布的 Release 说明（同时写进 `latest.json` 的 notes，供应用内更新弹框展示）。

用法（CI 里一行）：
    python scripts/gen_release_notes.py            # 追加到 $GITHUB_OUTPUT（step output: body）
    python scripts/gen_release_notes.py --print    # 直接打印，本地预览

为什么用脚本而不是 YAML 里写 heredoc：`run: |` 的内容必须缩进，
而 heredoc 结束符要求顶格，两者冲突（见 tasks/lessons.md 2026-11-06 第 3 条）。
"""

from __future__ import annotations

import os
import subprocess
import sys

MAX_LINES = 40

FOOTER = (
    "Windows / Linux / macOS 安装包与 `latest.json` 由 CI 产出；"
    "`.sig` 用于客户端校验，请勿单独分发安装包。\n"
    "macOS 仅提供 Apple Silicon（aarch64），未签名，首次打开需右键 → 打开。"
)


def git(*args: str) -> str:
    # Windows runner 的默认编码不是 UTF-8：中文提交信息会在这里抛 UnicodeDecodeError，
    # 显式指定 utf-8 并允许替换，宁可丢一两个字符也不能让发版失败。
    result = subprocess.run(
        ["git", *args],
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return result.stdout.strip()


def previous_tag() -> str:
    """上一个 tag；新建仓库/首个版本时为空。"""
    return git("describe", "--tags", "--abbrev=0", "HEAD^")


def commits(prev: str) -> list[str]:
    scope = f"{prev}..HEAD" if prev else "HEAD"
    raw = git("log", "--no-merges", "--pretty=format:%s", scope)
    lines = [line.strip() for line in raw.splitlines() if line.strip()]
    # 文档/回顾类提交对用户没意义，过滤掉，避免「本次更新」里出现 docs(todo)
    keep = [
        line
        for line in lines
        if not line.lower().startswith(("docs", "chore: release", "Merge "))
    ]
    return (keep or lines)[:MAX_LINES]


def build_body() -> str:
    version = (
        os.environ.get("RELEASE_TAG", "")
        or os.environ.get("GITHUB_REF_NAME", "")
        or "unreleased"
    )
    prev = previous_tag()
    lines = commits(prev)
    head = f"### {version} 更新内容"
    if prev:
        head += f"（自 {prev} 起）"
    items = "\n".join(f"- {line}" for line in lines) if lines else "- 维护性更新"
    return f"{head}\n\n{items}\n\n{FOOTER}"


FALLBACK = (
    "本次更新包含功能改进与问题修复。\n\n"
    "Windows / Linux / macOS 安装包与 `latest.json` 由 CI 产出；"
    "`.sig` 用于客户端校验，请勿单独分发安装包。"
)


def main() -> int:
    # 发布说明是锦上添花，绝不能因为它让整个发版失败：任何异常都退回通用文案。
    try:
        body = build_body()
    except Exception as error:  # noqa: BLE001 - 兜底就是为了吞掉一切
        print(f"release notes fallback: {error}", file=sys.stderr)
        body = FALLBACK
    if "--print" in sys.argv:
        print(body)
        return 0
    output = os.environ.get("GITHUB_OUTPUT")
    if not output:
        print(body)
        return 0
    with open(output, "a", encoding="utf-8") as handle:
        handle.write("body<<WB_RELEASE_NOTES_EOF\n")
        handle.write(body)
        handle.write("\nWB_RELEASE_NOTES_EOF\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
