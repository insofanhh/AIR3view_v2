"""Build the AIR3view geometric video-review mark as SVG, PNG and Windows ICO."""
from pathlib import Path
from PIL import Image, ImageDraw

root = Path(__file__).resolve().parent.parent / "assets"
root.mkdir(exist_ok=True)
size = 1024
image = Image.new("RGBA", (size, size))
mask = Image.new("L", (size, size))
ImageDraw.Draw(mask).rounded_rectangle((16, 16, 1008, 1008), radius=224, fill=255)
for y in range(size):
    fraction = y / (size - 1)
    color = tuple(round(a + (b-a)*fraction) for a, b in zip((45, 130, 255), (23, 58, 170)))
    ImageDraw.Draw(image).line((0, y, size, y), fill=(*color, 255))
image.putalpha(mask)
draw = ImageDraw.Draw(image)
draw.rounded_rectangle((200, 236, 824, 710), radius=100, fill="white")
draw.polygon([(300, 680), (300, 808), (460, 700)], fill="white")
draw.polygon([(444, 360), (444, 600), (644, 480)], fill=(34, 102, 231, 255))
for x, top in [(640, 736), (708, 710), (776, 684)]:
    draw.rounded_rectangle((x, top, x+46, 852), radius=14, fill=(51, 235, 211, 255))
image = image.resize((512, 512), Image.Resampling.LANCZOS)
image.save(root / "air3view.png")
image.save(root / "air3view.ico", sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
(root / "air3view.svg").write_text('''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
<defs><linearGradient id="blue" x2="0" y2="1"><stop stop-color="#2d82ff"/><stop offset="1" stop-color="#173aaa"/></linearGradient></defs>
<rect x="8" y="8" width="496" height="496" rx="112" fill="url(#blue)"/>
<rect x="100" y="118" width="312" height="237" rx="50" fill="white"/>
<path d="M150 340v64l80-54" fill="white"/>
<path d="M222 180v120l100-60z" fill="#2266e7"/>
<g fill="#33ebd3"><rect x="320" y="368" width="23" height="58" rx="7"/><rect x="354" y="355" width="23" height="71" rx="7"/><rect x="388" y="342" width="23" height="84" rx="7"/></g></svg>
''', encoding="utf-8")
print(f"Created AIR3view icon assets in {root}")
