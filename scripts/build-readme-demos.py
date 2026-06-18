from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageOps


CANVAS = (1080, 608)
FPS_MS = 85


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    names = ["msyhbd.ttc", "msyh.ttc"] if bold else ["msyh.ttc", "msyhbd.ttc"]
    for name in names:
        path = Path("C:/Windows/Fonts") / name
        if path.exists():
            return ImageFont.truetype(str(path), size)
    return ImageFont.load_default()


def cover(image: Image.Image, size: tuple[int, int]) -> Image.Image:
    scale = max(size[0] / image.width, size[1] / image.height)
    resized = image.resize((int(image.width * scale), int(image.height * scale)), Image.Resampling.LANCZOS)
    left = (resized.width - size[0]) // 2
    top = (resized.height - size[1]) // 2
    return resized.crop((left, top, left + size[0], top + size[1]))


def rounded_window(screenshot: Image.Image, width: int = 940) -> Image.Image:
    ratio = width / screenshot.width
    height = int(screenshot.height * ratio)
    shot = screenshot.convert("RGB").resize((width, height), Image.Resampling.LANCZOS)
    mask = Image.new("L", shot.size)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, shot.width - 1, shot.height - 1), radius=15, fill=255)
    result = Image.new("RGBA", shot.size)
    result.paste(shot, mask=mask)
    return result


def scene(background: Image.Image, screenshot: Image.Image, label: str, accent: tuple[int, int, int], offset_y: int = 30) -> Image.Image:
    toned = ImageOps.colorize(
        ImageOps.grayscale(cover(background, CANVAS)),
        black=(8, 12, 25),
        white=(119, 129, 168),
        mid=(39, 49, 82),
        blackpoint=12,
        whitepoint=242,
        midpoint=138,
    )
    frame = toned.convert("RGBA")
    frame = frame.filter(ImageFilter.GaussianBlur(4))
    shade = Image.new("RGBA", CANVAS, (7, 12, 24, 78))
    frame.alpha_composite(shade)
    window = rounded_window(screenshot)
    x = (CANVAS[0] - window.width) // 2
    y = offset_y
    shadow = Image.new("RGBA", CANVAS)
    shadow_draw = ImageDraw.Draw(shadow)
    shadow_draw.rounded_rectangle((x - 10, y + 10, x + window.width + 10, y + window.height + 25), 24, fill=(0, 0, 0, 115))
    shadow = shadow.filter(ImageFilter.GaussianBlur(24))
    frame.alpha_composite(shadow)
    frame.alpha_composite(window, (x, y))
    draw = ImageDraw.Draw(frame)
    draw.rounded_rectangle((42, 38, 185, 78), 18, fill=(*accent, 235))
    draw.text((61, 48), label, font=font(19, True), fill="white")
    return frame


def fade(first: Image.Image, second: Image.Image, count: int = 8) -> list[Image.Image]:
    return [Image.blend(first, second, index / count).convert("RGB") for index in range(1, count + 1)]


def save_gif(frames: list[Image.Image], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    frames[0].save(
        path,
        save_all=True,
        append_images=frames[1:],
        duration=FPS_MS,
        loop=0,
        optimize=True,
        disposal=2,
    )


def overview(background: Image.Image, shots: list[Image.Image], output: Path) -> None:
    scenes = [
        scene(background, shots[0], "Agent", (45, 112, 224)),
        scene(background, shots[1], "Code", (91, 73, 217)),
        scene(background, shots[2], "Office", (217, 88, 72)),
    ]
    frames: list[Image.Image] = []
    for index, current in enumerate(scenes):
        frames.extend([current.convert("RGB")] * 12)
        frames.extend(fade(current, scenes[(index + 1) % len(scenes)]))
    save_gif(frames, output)


def agent_demo(background: Image.Image, screenshot: Image.Image, output: Path) -> None:
    base = scene(background, screenshot, "Agent", (45, 112, 224))
    frames: list[Image.Image] = []
    prompt = "分析当前仓库，修复登录流程并补全测试"
    for length in range(0, len(prompt) + 1, 2):
        frame = base.copy()
        draw = ImageDraw.Draw(frame)
        draw.rounded_rectangle((335, 493, 820, 557), 18, fill=(255, 255, 255, 244), outline=(215, 223, 235, 255), width=2)
        draw.text((360, 514), prompt[:length] + ("|" if length < len(prompt) else ""), font=font(16), fill=(37, 43, 55))
        frames.append(frame.convert("RGB"))
    reply = ["已读取项目结构", "定位到鉴权状态不同步", "完成修复并补充 6 个测试", "全部检查通过"]
    for count in range(len(reply) + 1):
        frame = base.copy()
        draw = ImageDraw.Draw(frame)
        draw.rounded_rectangle((350, 210, 820, 430), 20, fill=(255, 255, 255, 246), outline=(219, 225, 235, 255), width=2)
        draw.text((376, 234), "Lan Code 正在处理", font=font(18, True), fill=(31, 42, 60))
        for index, line in enumerate(reply[:count]):
            y = 280 + index * 34
            draw.ellipse((378, y + 3, 392, y + 17), fill=(46, 163, 102))
            draw.text((406, y), line, font=font(15), fill=(54, 62, 77))
        frames.extend([frame.convert("RGB")] * 5)
    save_gif(frames + [frames[-1]] * 10, output)


def code_demo(background: Image.Image, screenshot: Image.Image, output: Path) -> None:
    base = scene(background, screenshot, "Code", (91, 73, 217))
    frames: list[Image.Image] = []
    typed = "async function saveTask(task) {"
    completion = "\n  const result = await api('/tasks', { method: 'POST', body: JSON.stringify(task) });\n  return result;\n}"
    for length in range(0, len(typed) + 1, 2):
        frame = base.copy()
        draw = ImageDraw.Draw(frame)
        draw.rounded_rectangle((250, 207, 840, 330), 14, fill=(21, 28, 42, 235), outline=(88, 104, 138, 220), width=2)
        draw.text((276, 232), typed[:length] + "|", font=font(16), fill=(169, 205, 255))
        frames.append(frame.convert("RGB"))
    for alpha in (65, 100, 140, 185):
        frame = base.copy()
        overlay = Image.new("RGBA", CANVAS)
        draw = ImageDraw.Draw(overlay)
        draw.rounded_rectangle((250, 207, 840, 360), 14, fill=(21, 28, 42, 242), outline=(88, 104, 138, 220), width=2)
        draw.text((276, 232), typed, font=font(16), fill=(169, 205, 255))
        draw.multiline_text((276, 262), completion, font=font(15), fill=(180, 190, 208, alpha), spacing=6)
        frame.alpha_composite(overlay)
        frames.extend([frame.convert("RGB")] * 4)
    accepted = base.copy()
    draw = ImageDraw.Draw(accepted)
    draw.rounded_rectangle((250, 207, 840, 360), 14, fill=(21, 28, 42, 242), outline=(74, 183, 142, 230), width=2)
    draw.text((276, 232), typed, font=font(16), fill=(169, 205, 255))
    draw.multiline_text((276, 262), completion, font=font(15), fill=(191, 230, 210), spacing=6)
    draw.rounded_rectangle((690, 316, 810, 346), 12, fill=(47, 153, 105))
    draw.text((718, 322), "Tab 接受", font=font(13, True), fill="white")
    frames.extend([accepted.convert("RGB")] * 14)
    save_gif(frames, output)


def office_demo(background: Image.Image, screenshot: Image.Image, output: Path) -> None:
    base = scene(background, screenshot, "Office", (217, 88, 72))
    frames: list[Image.Image] = []
    steps = [
        ("理解需求", "提取主题、受众和表达重点", 22),
        ("生成结构", "建立 8 页演示文稿大纲", 48),
        ("设计页面", "统一字体、色彩、图表和留白", 78),
        ("完成交付", "PPTX 已生成，可继续编辑", 100),
    ]
    for title, subtitle, progress in steps:
        frame = base.copy()
        draw = ImageDraw.Draw(frame)
        draw.rounded_rectangle((278, 186, 817, 432), 22, fill=(255, 255, 255, 246), outline=(225, 221, 219, 255), width=2)
        draw.text((310, 214), "Lan Code AI 演示文稿", font=font(23, True), fill=(37, 40, 48))
        draw.text((310, 257), title, font=font(18, True), fill=(205, 76, 58))
        draw.text((310, 290), subtitle, font=font(15), fill=(86, 91, 102))
        draw.rounded_rectangle((310, 346, 775, 362), 8, fill=(235, 229, 227))
        draw.rounded_rectangle((310, 346, 310 + int(465 * progress / 100), 362), 8, fill=(221, 91, 69))
        draw.text((310, 380), f"{progress}% · 正在写入新演示.pptx", font=font(13), fill=(112, 117, 126))
        frames.extend([frame.convert("RGB")] * 9)
    save_gif(frames + [frames[-1]] * 12, output)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--background", required=True)
    parser.add_argument("--agent", required=True)
    parser.add_argument("--code", required=True)
    parser.add_argument("--office", required=True)
    parser.add_argument("--output", default="docs/assets")
    args = parser.parse_args()
    background = Image.open(args.background).convert("RGB")
    shots = [Image.open(path).convert("RGBA") for path in (args.agent, args.code, args.office)]
    output = Path(args.output)
    overview(background, shots, output / "lan-code-overview.gif")
    agent_demo(background, shots[0], output / "lan-code-agent.gif")
    code_demo(background, shots[1], output / "lan-code-code.gif")
    office_demo(background, shots[2], output / "lan-code-office.gif")


if __name__ == "__main__":
    main()
