"""Genera los avatares que trae el server de fábrica (backend/server/src/avatars.rs, `seed_defaults`).
Diseños propios, simples y de colores planos, para que se lean bien chicos (barra lateral) y en la
tele ("Who's watching?"). Se dibujan a 1024 px y se achican a 256 (bordes suaves).

    python backend/server/assets/avatars/make.py      # reescribe los PNG de esta carpeta

Si se cambia un dibujo, el PNG nuevo solo llega a servers nuevos: los que ya sembraron la galería
no la vuelven a sembrar (ver `seed_defaults`)."""
import math, os
from PIL import Image, ImageDraw

S = 1024
OUT = os.path.dirname(os.path.abspath(__file__))
INK = (24, 24, 32)
WHITE = (250, 250, 250)


def canvas(bg):
    im = Image.new("RGB", (S, S), bg)
    return im, ImageDraw.Draw(im)


def circle(d, cx, cy, r, fill):
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=fill)


def smile(d, cx, cy, w, h, width=46, fill=INK):
    d.arc([cx - w, cy - h, cx + w, cy + h], 20, 160, fill=fill, width=width)


def eyes(d, y, dx=150, r=52, fill=INK, cx=S // 2):
    circle(d, cx - dx, y, r, fill)
    circle(d, cx + dx, y, r, fill)


def save(im, name):
    im.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, name + ".png"), optimize=True)


def a_smile():
    im, d = canvas((52, 120, 246))
    eyes(d, 430)
    smile(d, 512, 560, 230, 170)
    save(im, "01-smile")


def a_wink():
    im, d = canvas((232, 68, 76))
    circle(d, 362, 430, 52, INK)
    d.line([600, 430, 720, 430], fill=INK, width=46)
    smile(d, 512, 560, 230, 170)
    circle(d, 300, 600, 55, (255, 140, 150))
    circle(d, 724, 600, 55, (255, 140, 150))
    save(im, "02-wink")


def a_cat():
    im, d = canvas((246, 146, 48))
    d.polygon([(190, 360), (260, 120), (420, 290)], fill=INK)
    d.polygon([(834, 360), (764, 120), (604, 290)], fill=INK)
    d.polygon([(235, 330), (275, 200), (360, 290)], fill=(255, 196, 140))
    d.polygon([(789, 330), (749, 200), (664, 290)], fill=(255, 196, 140))
    eyes(d, 480, dx=170, r=56)
    d.polygon([(470, 580), (554, 580), (512, 630)], fill=INK)
    for dy in (-30, 30):
        d.line([120, 620 + dy, 360, 610], fill=INK, width=16)
        d.line([904, 620 + dy, 664, 610], fill=INK, width=16)
    d.arc([420, 600, 512, 700], 0, 180, fill=INK, width=22)
    d.arc([512, 600, 604, 700], 0, 180, fill=INK, width=22)
    save(im, "03-cat")


def a_robot():
    im, d = canvas((32, 178, 170))
    d.line([512, 120, 512, 250], fill=INK, width=30)
    circle(d, 512, 110, 46, (255, 214, 64))
    d.rounded_rectangle([220, 250, 804, 820], radius=90, fill=(222, 236, 238))
    d.rounded_rectangle([300, 380, 450, 500], radius=26, fill=INK)
    d.rounded_rectangle([574, 380, 724, 500], radius=26, fill=INK)
    circle(d, 400, 430, 22, (90, 220, 255))
    circle(d, 674, 430, 22, (90, 220, 255))
    d.rounded_rectangle([330, 620, 694, 700], radius=20, fill=INK)
    for x in range(380, 680, 70):
        d.line([x, 620, x, 700], fill=(222, 236, 238), width=14)
    d.rectangle([160, 450, 220, 600], fill=INK)
    d.rectangle([804, 450, 864, 600], fill=INK)
    save(im, "04-robot")


def a_ghost():
    im, d = canvas((124, 82, 220))
    body = [(250, 520)]
    d.pieslice([250, 170, 774, 694], 180, 360, fill=WHITE)
    d.rectangle([250, 430, 774, 820], fill=WHITE)
    for i in range(4):
        x0 = 250 + i * 131
        d.pieslice([x0, 760, x0 + 131, 880], 0, 180, fill=WHITE)
    del body
    circle(d, 420, 470, 50, INK)
    circle(d, 604, 470, 50, INK)
    d.ellipse([470, 560, 554, 660], fill=INK)
    save(im, "05-ghost")


def a_alien():
    im, d = canvas((26, 34, 64))
    for x, y, r in ((140, 160, 10), (860, 220, 14), (180, 860, 12), (880, 800, 9), (760, 120, 8)):
        circle(d, x, y, r, (255, 240, 160))
    d.ellipse([230, 180, 794, 880], fill=(120, 220, 110))
    for sx in (-1, 1):
        cx = 512 + sx * 140
        pts = []
        for k in range(36):
            t = 2 * math.pi * k / 36
            px, py = 95 * math.cos(t), 150 * math.sin(t)
            ang = sx * math.radians(30)
            pts.append((cx + px * math.cos(ang) - py * math.sin(ang), 470 + px * math.sin(ang) + py * math.cos(ang)))
        d.polygon(pts, fill=INK)
        circle(d, cx - sx * 20, 420, 26, WHITE)
    d.arc([430, 640, 594, 740], 20, 160, fill=INK, width=24)
    save(im, "06-alien")


def a_bear():
    im, d = canvas((255, 196, 54))
    brown = (150, 92, 52)
    circle(d, 270, 280, 130, brown)
    circle(d, 754, 280, 130, brown)
    circle(d, 270, 280, 66, (220, 160, 110))
    circle(d, 754, 280, 66, (220, 160, 110))
    circle(d, 512, 560, 350, brown)
    d.ellipse([362, 560, 662, 800], fill=(236, 196, 150))
    eyes(d, 470, dx=140, r=40)
    d.ellipse([462, 590, 562, 660], fill=INK)
    d.arc([440, 630, 512, 720], 10, 170, fill=INK, width=18)
    d.arc([512, 630, 584, 720], 10, 170, fill=INK, width=18)
    save(im, "07-bear")


def a_shades():
    im, d = canvas((240, 98, 170))
    d.rounded_rectangle([180, 360, 480, 520], radius=60, fill=INK)
    d.rounded_rectangle([544, 360, 844, 520], radius=60, fill=INK)
    d.rectangle([470, 400, 554, 440], fill=INK)
    d.line([250, 400, 330, 470], fill=(110, 110, 140), width=20)
    d.line([614, 400, 694, 470], fill=(110, 110, 140), width=20)
    d.pieslice([300, 520, 724, 800], 0, 180, fill=INK)
    d.rectangle([340, 660, 684, 700], fill=WHITE)
    save(im, "08-shades")


def a_sleepy():
    im, d = canvas((70, 72, 170))
    d.arc([262, 400, 462, 520], 200, 340, fill=WHITE, width=40)
    d.arc([562, 400, 762, 520], 200, 340, fill=WHITE, width=40)
    d.ellipse([462, 600, 562, 690], fill=WHITE)
    for (x, y, s) in ((720, 260, 110), (820, 150, 80), (890, 70, 56)):
        w = max(10, s // 6)
        d.line([x, y, x + s, y], fill=(190, 200, 255), width=w)
        d.line([x + s, y, x, y + s], fill=(190, 200, 255), width=w)
        d.line([x, y + s, x + s, y + s], fill=(190, 200, 255), width=w)
    save(im, "09-sleepy")


def a_monster():
    im, d = canvas((255, 128, 64))
    for x, y, r in ((300, 400, 95), (512, 330, 110), (724, 400, 95)):
        circle(d, x, y, r, WHITE)
        circle(d, x + 14, y + 16, r * 0.48, INK)
        circle(d, x - 8, y - 10, r * 0.16, WHITE)
    d.chord([250, 520, 774, 860], 0, 180, fill=INK)
    for x in (340, 460, 580):
        d.polygon([(x, 690), (x + 100, 690), (x + 50, 770)], fill=WHITE)
    save(im, "10-monster")


def a_panda():
    im, d = canvas((96, 190, 140))
    circle(d, 280, 260, 120, INK)
    circle(d, 744, 260, 120, INK)
    circle(d, 512, 560, 360, WHITE)
    for sx in (-1, 1):
        d.ellipse([512 + sx * 150 - 95, 380, 512 + sx * 150 + 95, 580], fill=INK)
        circle(d, 512 + sx * 140, 470, 36, WHITE)
        circle(d, 512 + sx * 140, 478, 18, INK)
    d.ellipse([462, 620, 562, 690], fill=INK)
    d.arc([440, 660, 512, 750], 10, 170, fill=INK, width=18)
    d.arc([512, 660, 584, 750], 10, 170, fill=INK, width=18)
    save(im, "11-panda")


def a_star():
    im, d = canvas((40, 170, 236))
    pts = []
    for k in range(10):
        r = 420 if k % 2 == 0 else 190
        t = -math.pi / 2 + k * math.pi / 5
        pts.append((512 + r * math.cos(t), 540 + r * math.sin(t)))
    d.polygon(pts, fill=(255, 214, 64))
    eyes(d, 520, dx=80, r=34)
    smile(d, 512, 600, 90, 60, width=24)
    save(im, "12-star")


if __name__ == "__main__":
    for f in (a_smile, a_wink, a_cat, a_robot, a_ghost, a_alien, a_bear, a_shades, a_sleepy, a_monster, a_panda, a_star):
        f()
    print("ok", OUT)
