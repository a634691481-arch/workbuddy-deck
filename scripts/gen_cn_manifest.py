"""把官方 latest.json 转成「国内镜像清单」。

规则：只改 `platforms.*.url` 里指向 github.com 的地址，前面加镜像前缀；
签名（signature）原样保留，客户端仍会按 minisign 校验，镜像无法伪造内容。

用法：python scripts/gen_cn_manifest.py latest.json latest-cn.json [mirror]
"""
import json
import sys

DEFAULT_MIRROR = "https://gh-proxy.com/"
GITHUB = "https://github.com/"


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: gen_cn_manifest.py <in.json> <out.json> [mirror]", file=sys.stderr)
        return 2
    src, dst = sys.argv[1], sys.argv[2]
    mirror = sys.argv[3] if len(sys.argv) > 3 else DEFAULT_MIRROR
    if not mirror.endswith("/"):
        mirror += "/"

    with open(src, encoding="utf-8") as handle:
        data = json.load(handle)

    platforms = data.get("platforms") or {}
    rewritten = 0
    for item in platforms.values():
        url = item.get("url", "")
        if url.startswith(GITHUB):
            item["url"] = mirror + url
            rewritten += 1

    with open(dst, "w", encoding="utf-8") as handle:
        json.dump(data, handle, ensure_ascii=False, indent=2)

    print(f"version={data.get('version')} platforms={list(platforms)} rewritten={rewritten}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
