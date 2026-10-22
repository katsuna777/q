from PIL import Image, ImageDraw, ImageFilter
import math, os, sys
OUT = sys.argv[1]
N = 2048  # draw at 2x, downsample for smooth edges
S = N / 1024

def rr(d, box, r, fill):
    d.rounded_rectangle([v * S for v in box], radius=r * S, fill=fill)

img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
# background: rounded square with vertical gradient
grad = Image.new("RGBA", (N, N))
gd = ImageDraw.Draw(grad)
top, bot = (43, 58, 103), (17, 24, 48)
for y in range(N):
    t = y / (N - 1)
    gd.line([(0, y), (N, y)], fill=tuple(int(top[i] + (bot[i] - top[i]) * t) for i in range(3)) + (255,))
mask = Image.new("L", (N, N), 0)
ImageDraw.Draw(mask).rounded_rectangle([64 * S, 64 * S, 960 * S, 960 * S], radius=200 * S, fill=255)
img.paste(grad, (0, 0), mask)
d = ImageDraw.Draw(img)
d.rounded_rectangle([64.5 * S, 64.5 * S, 959.5 * S, 959.5 * S], radius=199.5 * S, outline=(255, 255, 255, 22), width=int(3 * S))

# calendar sheet (soft shadow first)
sh = Image.new("RGBA", (N, N), (0, 0, 0, 0))
ImageDraw.Draw(sh).rounded_rectangle([232 * S, 262 * S, 792 * S, 842 * S], radius=70 * S, fill=(0, 0, 0, 120))
sh = sh.filter(ImageFilter.GaussianBlur(26 * S))
img.alpha_composite(sh)
d = ImageDraw.Draw(img)
rr(d, (232, 232, 792, 792), 70, (244, 246, 255, 255))
# header band (orange) with square bottom corners
band = Image.new("RGBA", (N, N), (0, 0, 0, 0))
bd = ImageDraw.Draw(band)
bd.rounded_rectangle([232 * S, 232 * S, 792 * S, 392 * S], radius=70 * S, fill=(255, 138, 76, 255))
bd.rectangle([232 * S, 330 * S, 792 * S, 392 * S], fill=(255, 138, 76, 255))
img.alpha_composite(band)
d = ImageDraw.Draw(img)
# binder rings
for x in (360, 664):
    rr(d, (x - 22, 178, x + 22, 300), 22, (244, 246, 255, 255))
    rr(d, (x - 10, 196, x + 10, 282), 10, (17, 24, 48, 255))

# sun with rays over a wave line (the "vacation" mark)
cx, cy, r = 512, 585, 92
for k in range(1, 8):
    a = math.pi + k * math.pi / 8
    x1, y1 = cx + math.cos(a) * (r + 30), cy + math.sin(a) * (r + 30)
    x2, y2 = cx + math.cos(a) * (r + 74), cy + math.sin(a) * (r + 74)
    d.line([(x1 * S, y1 * S), (x2 * S, y2 * S)], fill=(255, 184, 77, 255), width=int(22 * S))
    for (x, y) in ((x1, y1), (x2, y2)):
        d.ellipse([(x - 11) * S, (y - 11) * S, (x + 11) * S, (y + 11) * S], fill=(255, 184, 77, 255))
d.pieslice([(cx - r) * S, (cy - r) * S, (cx + r) * S, (cy + r) * S], 180, 360, fill=(255, 184, 77, 255))
# waves
for i, y in enumerate((590, 650, 710)):
    pts = []
    for xi in range(300, 725, 6):
        pts.append((xi * S, (y + 11 * math.sin((xi - 300) / 34.0 + i)) * S))
    d.line(pts, fill=(60, 110, 210, 255) if i == 0 else (60, 110, 210, 190 - i * 40), width=int(20 * S), joint="curve")
img = img.resize((1024, 1024), Image.LANCZOS)
os.makedirs(OUT, exist_ok=True)
img.save(f"{OUT}/icon-1024.png")
img.resize((512, 512), Image.LANCZOS).save(f"{OUT}/icon.png")
# .ico (Windows)
img.save(f"{OUT}/icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
# .icns (macOS) via iconset
iset = f"{OUT}/icon.iconset"
os.makedirs(iset, exist_ok=True)
for base in (16, 32, 128, 256, 512):
    img.resize((base, base), Image.LANCZOS).save(f"{iset}/icon_{base}x{base}.png")
    img.resize((base * 2, base * 2), Image.LANCZOS).save(f"{iset}/icon_{base}x{base}@2x.png")
print("ok")
