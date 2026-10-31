"""15 秒功能演示片渲染：真实截图满幅 + 缓推/拉远 + 关键区高亮 + 左下文字条。

不引第三方库，PIL 逐帧画、ffmpeg 合成。输出 mp4（1680×1170@15fps）与 README 用的 GIF。
"""
import math
import subprocess
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent
SHOTS = ROOT.parent.parent / "docs" / "screenshots"
FRAMES = ROOT / "frames"
OUT_MP4 = ROOT / "feature-tour.mp4"
OUT_GIF = ROOT.parent.parent / "docs" / "screenshots" / "feature-tour.gif"

W, H = 1680, 1170
FPS = 15
SEG = 3.75  # 每镜秒数
SEGS = 4

FG = (234, 234, 234)
SUB = (150, 150, 150)
RED = (230, 25, 25)
SHADE = (0, 0, 0, 205)

ZH = "C:/Windows/Fonts/msyh.ttc"
MONO = "C:/Windows/Fonts/consola.ttf"


def ease_out(t: float) -> float:
    return 1 - (1 - t) ** 3


def ease_in_out(t: float) -> float:
    return 3 * t * t - 2 * t * t * t


def crop_zoom(img: Image.Image, zoom: float, cx: float, cy: float):
    """按 zoom 从原图裁一块再放大到画布；返回画布与裁剪原点，供高亮做同一套坐标变换。"""
    w, h = W / zoom, H / zoom
    left = min(max(cx * W - w / 2, 0), W - w)
    top = min(max(cy * H - h / 2, 0), H - h)
    box = (round(left), round(top), round(left + w), round(top + h))
    return img.crop(box).resize((W, H), Image.LANCZOS), left, top


def draw_caption(draw: ImageDraw.ImageDraw, title: str, sub: str, alpha: float):
    """左下角文字块：红色竖条 + 半透明黑底，只占文字所需宽度。"""
    a = int(255 * alpha)
    f1 = ImageFont.truetype(ZH, 30)
    f2 = ImageFont.truetype(ZH, 19)
    bh = 74
    y = H - bh
    draw.rectangle([0, y, W, H], fill=(0, 0, 0, int(228 * alpha)))
    draw.rectangle([0, y, W, y + 2], fill=RED + (int(160 * alpha),))
    draw.rectangle([48, y + 16, 54, y + bh - 16], fill=RED + (a,))
    draw.text((76, y + 11), title, font=f1, fill=FG + (a,))
    draw.text((76, y + 43), sub, font=f2, fill=SUB + (a,))


def draw_highlight(draw, rect, alpha: float, left: float, top: float, zoom: float, lw: int = 3):
    """rect 是原图归一化坐标；换算到当前缩放后的画布，避免框跟着内容一起漂。"""
    x, y, w, h = rect
    box = [(x * W - left) * zoom, (y * H - top) * zoom,
           ((x + w) * W - left) * zoom, ((y + h) * H - top) * zoom]
    draw.rectangle(box, outline=RED + (int(255 * alpha),), width=lw)


# 四镜：图 / 焦点 / 运镜 / 文字 / 高亮区（归一化）
SHOTS_CFG = [
    {
        "file": "01-accounts.png",
        "focus": (0.5, 0.44),
        "zoom": lambda t: 1.0 + 0.05 * ease_out(t),
        "title": "多账号 · 一屏总览",
        "sub": "余额 / 到期 / 限流标记，谁最紧一眼看出",
        "hl": (0.062, 0.16, 0.918, 0.62),
        "hl_at": 0.35,
    },
    {
        "file": "02-tasks.png",
        "focus": (0.55, 0.42),
        "zoom": lambda t: 1.05 - 0.05 * ease_in_out(t),
        "title": "签到 · 成长任务 · 旅行",
        "sub": "批量执行，逐账号回执；失败聚合条单独列出",
        "hl": (0.28, 0.28, 0.69, 0.5),
        "hl_at": 0.3,
    },
    {
        "file": "03-credits.png",
        "focus": (0.5, 0.5),
        "zoom": lambda t: 1.02,
        "pan": lambda t: (0.5 - 0.012 * ease_in_out(t), 0.5),
        "title": "积分与消耗",
        "sub": "官方口径优先，取不到就回退本地快照",
        "hl": None,
        "hl_at": 9,
    },
    {
        "file": "04-tokens.png",
        "focus": (0.5, 0.5),
        "zoom": lambda t: 1.0 + 0.03 * ease_out(t),
        "title": "TOKEN 账本 · 四源日志",
        "sub": "按模型 / 项目 / 会话拆开；日志含切换台账",
        "hl": None,
        "hl_at": 9,
    },
]


def main():
    FRAMES.mkdir(exist_ok=True)
    for old in FRAMES.glob("*.png"):
        old.unlink()

    per = int(SEG * FPS)
    idx = 0
    for shot in SHOTS_CFG:
        src = Image.open(SHOTS / shot["file"]).convert("RGB")
        if src.size != (W, H):
            src = src.resize((W, H), Image.LANCZOS)
        for f in range(per):
            t = f / max(per - 1, 1)
            zoom = shot["zoom"](t)
            cx, cy = shot.get("pan", lambda _t: shot["focus"])(t) if "pan" in shot else shot["focus"]
            base = src
            if shot["hl"] and t >= shot["hl_at"]:
                a = min(1.0, (t - shot["hl_at"]) / 0.18)
                base = src.copy()
                dx, dy, dw, dh = shot["hl"]
                ImageDraw.Draw(base).rectangle(
                    [dx * W, dy * H, (dx + dw) * W, (dy + dh) * H],
                    outline=RED + (int(255 * a),), width=5,
                )
            frame, left, top = crop_zoom(base, zoom, cx, cy)
            frame = frame.convert("RGBA")
            layer = Image.new("RGBA", frame.size, (0, 0, 0, 0))
            d = ImageDraw.Draw(layer)
            cap_a = min(1.0, t / 0.16)
            draw_caption(d, shot["title"], shot["sub"], cap_a)
            Image.alpha_composite(frame, layer).convert("RGB").save(FRAMES / f"{idx:04d}.png")
            idx += 1

    print("frames:", idx)

    subprocess.run(
        [
            "ffmpeg", "-y", "-v", "error", "-framerate", str(FPS),
            "-i", str(FRAMES / "%04d.png"),
            "-c:v", "libx264", "-crf", "17", "-pix_fmt", "yuv420p", "-an", str(OUT_MP4),
        ],
        check=True,
    )
    print("mp4:", OUT_MP4.stat().st_size // 1024, "KB")

    # README 用 GIF：900 宽 / 12fps / 两遍调色板，控制体积
    palette = "palette.png"
    subprocess.run(
        ["ffmpeg", "-y", "-v", "error", "-i", str(OUT_MP4), "-vf",
         "fps=10,scale=820:-1:flags=lanczos,palettegen=max_colors=128:stats_mode=diff", palette],
        check=True,
    )
    subprocess.run(
        ["ffmpeg", "-y", "-v", "error", "-i", str(OUT_MP4), "-i", palette, "-lavfi",
         "fps=10,scale=820:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=none",
         "-loop", "0", str(OUT_GIF)],
        check=True,
    )
    print("gif:", OUT_GIF.stat().st_size // 1024, "KB")
    Path(palette).unlink(missing_ok=True)


if __name__ == "__main__":
    main()
