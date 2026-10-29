"""Deterministic 2D character rig driven by Rhubarb visemes; streaming RGBA -> FFmpeg.

Original procedural artwork, no downloaded character or paid generation service.
"""
import argparse
import bisect
import json
import math
import subprocess
from functools import lru_cache
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw
from gesture_rig import draw_half_body, draw_robe_body, plan_gestures

SHAPES = {"X": (19, 1), "A": (18, 2), "B": (23, 7), "C": (23, 16),
          "D": (24, 25), "E": (16, 17), "F": (10, 13), "G": (21, 9), "H": (22, 19)}


@lru_cache(maxsize=1)
def panda_base():
    # This fixed sprite has an intentionally empty mouth area below the nose.
    with Image.open(Path(__file__).with_name("panda-head.png")) as source:
        source = source.convert("RGBA")
        source.thumbnail((276, 276), Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (512, 512))
        canvas.alpha_composite(source, ((512-source.width)//2, 5))
        return canvas


def panda_frame(t, cues, emotion, size, gestures):
    head = panda_base().copy()
    draw = ImageDraw.Draw(head)
    shape, (mw, mh) = mouth_at(cues, t)
    x, y = 256, 221
    mw, mh = mw*.85, mh*.85
    if mh < 4:
        curve = [(x-mw+2*mw*i/24, y+4*(1-((i-12)/12)**2)) for i in range(25)]
        draw.line(curve, fill="#17171C", width=3, joint="curve")
    else:
        draw.ellipse((x-mw, y-mh*.5, x+mw, y+mh), fill="#34212A", outline="#17171C", width=3)
        if shape in ["B", "C", "D", "G", "H"]:
            draw.rounded_rectangle((x-mw+4, y-mh*.5+3, x+mw-4, y-mh*.5+7), 2, fill="#FFFCFA")
        if shape in ["C", "D", "H"]:
            draw.ellipse((x-mw*.5, y+mh*.4, x+mw*.5, y+mh*.85), fill="#CD7581")
    angle = math.sin(t*2.4)*.9
    if emotion == "surprised":
        angle += math.sin(t*8)*1.8*max(0, 1-t/.6)
    head = head.rotate(angle, Image.Resampling.BICUBIC, center=(256, 242),
                       expand=False, fillcolor=(0, 0, 0, 0))
    # Keep the existing shoulder/face rig at its original scale; extend the
    # canvas downward rather than squeezing a full robe into the old square.
    image = Image.new("RGBA", (512, 704))
    draw_robe_body(ImageDraw.Draw(image), t, gestures)
    image.alpha_composite(head, (0, round(math.sin(t*3))))
    return image if size == 512 else image.resize(frame_dimensions("panda", size), Image.Resampling.LANCZOS)


def frame_dimensions(character, size):
    # Even dimensions remain compatible with downstream video encoders.
    height = 2*round(size*704/512/2) if character == "panda" else size
    return size, height


@lru_cache(maxsize=1)
def portrait_base():
    # Prepared portrait preserves the supplied illustration; no per-frame disk I/O.
    with Image.open(Path(__file__).with_name("portrait-base.png")) as source:
        source = source.convert("RGBA")
        source.thumbnail((488, 488), Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (512, 512))
        canvas.alpha_composite(source, ((512-source.width)//2, (512-source.height)//2))
        # Isolate the supplied head from its old short hoodie. The new torso
        # supplies the neck/hood/shoulders; keeping the old collar would create
        # a second body and put the arm anchors at the neck again.
        mask = Image.new("L", (512, 512))
        boundary = [(0, 0), (512, 0), (512, 270), (397, 270),
                    (390, 282), (380, 298), (370, 311), (350, 328),
                    (330, 339), (310, 347), (290, 353), (270, 356),
                    (250, 357), (230, 355), (210, 350), (190, 343),
                    (170, 333), (150, 318), (133, 299), (121, 280), (0, 280)]
        ImageDraw.Draw(mask).polygon(boundary, fill=255)
        canvas.putalpha(ImageChops.multiply(canvas.getchannel("A"), mask))
        return canvas


def portrait_frame(t, cues, emotion, size, gestures):
    image = portrait_base().copy()
    draw = ImageDraw.Draw(image)
    shape, (mw, mh) = mouth_at(cues, t)
    # Mouth anchor belongs to this fixed, prepared portrait, not arbitrary uploads.
    x, y = 259, 284
    mw, mh = mw*2.2, mh*1.65
    if mh < 6:
        curve = [(x-mw+2*mw*i/24, y+10*(1-((i-12)/12)**2)) for i in range(25)]
        draw.line(curve, fill="#080808", width=4, joint="curve")
    else:
        draw.ellipse((x-mw, y-mh*.5, x+mw, y+mh), fill="#281A20", outline="#080808", width=4)
        if shape in ["B", "C", "D", "G", "H"]:
            draw.rounded_rectangle((x-mw+9, y-mh*.5+4, x+mw-9, y-mh*.5+11), 3, fill="#FFFCFA")
        if shape in ["C", "D", "H"]:
            draw.ellipse((x-mw*.55, y+mh*.4, x+mw*.55, y+mh*.85), fill="#CD7581")
    # Gentle nod and a short surprised tilt without repainting the reference eyes.
    angle = math.sin(t*2.4)*.65
    if emotion == "surprised":
        angle += math.sin(t*8)*1.6*max(0, 1-t/.6)
    # Face identity/visemes stay in source coordinates; only the head nods.
    image = image.resize((338, 338), Image.Resampling.LANCZOS)
    head = Image.new("RGBA", (512, 512))
    head.alpha_composite(image, (87, 0))
    head = head.rotate(angle, Image.Resampling.BICUBIC, center=(256, 226),
                       expand=False, fillcolor=(0, 0, 0, 0))
    moved = Image.new("RGBA", (512, 512))
    draw_half_body(ImageDraw.Draw(moved), t, gestures, portrait=True)
    moved.alpha_composite(head, (0, round(math.sin(t*3))))
    return moved if size == 512 else moved.resize((size, size), Image.Resampling.LANCZOS)


def emotion_kind(text):
    text = text.lower()
    if any(x in text for x in ["shock", "surpris", "bất ngờ", "sốc", "giật", "kinh ngạc"]):
        return "surprised"
    if any(x in text for x in ["sad", "concerned", "tense", "somber", "sombre", "đồng cảm", "buồn", "trầm", "lo lắng"]):
        return "concerned"
    if any(x in text for x in ["serious", "nghiêm", "phân tích", "tỉnh táo"]):
        return "serious"
    return "curious"


def mouth_at(cues, t):
    index = bisect.bisect_right([c["start"] for c in cues], t) - 1
    if index < 0 or t >= cues[index]["end"]:
        return "X", SHAPES["X"]
    cue = cues[index]
    shape = cue["value"]
    previous = cues[index-1]["value"] if index > 0 and cues[index-1]["end"] >= cue["start"] - .02 else "X"
    mix = min(1, max(0, (t-cue["start"]) / .045))
    a, b = SHAPES[previous], SHAPES[shape]
    return shape, (a[0]+(b[0]-a[0])*mix, a[1]+(b[1]-a[1])*mix)


def frame(t, cues, emotion="curious", character="studio", size=512, gestures=None):
    """Draw at a fixed logical scale. No randomness; arbitrary timestamp is seek safe."""
    if gestures is None:
        gestures = plan_gestures("Lời bình giải thích tình huống", [], cues, emotion, "auto", max((c["end"] for c in cues), default=1))
    if character == "portrait":
        return portrait_frame(t, cues, emotion, size, gestures)
    if character == "panda":
        return panda_frame(t, cues, emotion, size, gestures)
    image = Image.new("RGBA", (512, 512), (0, 0, 0, 0))
    d = ImageDraw.Draw(image)
    ink = "#172235"
    skin = "#F1B78F"
    shirt = "#536EF6" if character == "studio" else "#287C88"
    bob = 2.8 * math.sin(t * 2.4)
    lean = 3 * math.sin(t * 1.7) + (5 * math.sin(t*18) * math.exp(-t*2) if emotion == "surprised" else 0)
    def ellipse(box, fill, outline=None, width=1):
        d.ellipse(tuple(round(v) for v in box), fill=fill, outline=outline, width=width)
    def line(points, fill=ink, width=5):
        d.line([(round(x), round(y)) for x, y in points], fill=fill, width=width)
    hx, hy = 256+lean, 202+bob
    ellipse((hx-104, hy-112, hx+104, hy+122), "#FFFFFF")
    ellipse((hx-108, hy-20, hx-77, hy+32), skin, ink, 4)
    ellipse((hx+77, hy-20, hx+108, hy+32), skin, ink, 4)
    ellipse((hx-94, hy-105, hx+94, hy+116), skin, ink, 5)
    # Hair, headphones and microphone form an original reviewer character.
    d.polygon([(hx-93,hy-20),(hx-95,hy-90),(hx-62,hy-127),(hx-19,hy-119),
               (hx+22,hy-139),(hx+45,hy-112),(hx+80,hy-110),(hx+100,hy-66),
               (hx+89,hy-12),(hx+67,hy-67),(hx+30,hy-78),(hx-12,hy-52),(hx-58,hy-74)], fill=ink)
    d.arc((hx-116,hy-135,hx+116,hy+78), 185, 355, fill=ink, width=13)
    for side in [-1, 1]:
        x = hx + side*100
        d.rounded_rectangle((x-14,hy-30,x+14,hy+44), 13, fill="#283951", outline="#7185A5", width=4)
    line([(hx+103,hy+28),(hx+99,hy+75),(hx+51,hy+83)], "#283951", 6)
    ellipse((hx+41,hy+76,hx+58,hy+89), ink)
    # Eyes/blink, eyebrows and facial emotion.
    blink = (t % 4.1 < .105 and t > .1)
    eye_h = 2 if blink else (20 if emotion == "surprised" else 14)
    for side in [-1, 1]:
        x = hx + side*39
        ellipse((x-22,hy-21-eye_h/2,x+22,hy-21+eye_h), "#FFFFFF", ink, 3)
        if not blink:
            ellipse((x-7,hy-23,x+7,hy-9), ink)
            ellipse((x-3,hy-22,x+1,hy-18), "#FFFFFF")
        brow_y = hy-58 if emotion == "surprised" else hy-46
        slope = side*8 if emotion == "concerned" else (-side*7 if emotion == "serious" else 0)
        line([(x-20,brow_y+slope),(x+18,brow_y-slope)], ink, 7)
        if character == "analyst":
            d.rounded_rectangle((x-29,hy-43,x+29,hy+5), 10, outline=ink, width=5)
    if character == "analyst":
        line([(hx-9,hy-26),(hx+9,hy-26)], ink, 4)
    line([(hx+2,hy+4),(hx-5,hy+25),(hx+8,hy+27)], "#CE8D69", 3)
    shape, (mw, mh) = mouth_at(cues, t)
    my = hy+64
    if mh < 3:
        line([(hx-mw,my),(hx+mw,my+1)], "#703D3E", 4)
    else:
        ellipse((hx-mw,my-mh/2,hx+mw,my+mh), "#562B38", "#703D3E", 3)
        if shape in ["B", "C", "D", "G", "H"]:
            d.rounded_rectangle((hx-mw+5,my-mh/2+3,hx+mw-5,my-mh/2+7), 2, fill="#FFF3E8")
        if shape in ["C", "D", "H"]:
            ellipse((hx-mw*.55,my+mh*.4,hx+mw*.55,my+mh*.9), "#E9818F")
    head = image.resize((389, 389), Image.Resampling.LANCZOS)
    image = Image.new("RGBA", (512, 512))
    draw_half_body(ImageDraw.Draw(image), t, gestures, shirt=shirt)
    image.alpha_composite(head, (61, -11))
    return image if size == 512 else image.resize((size, size), Image.Resampling.LANCZOS)


def render(job):
    cues = json.loads(Path(job["cues"]).read_text(encoding="utf-8"))["mouthCues"]
    assert all(c["value"] in SHAPES and 0 <= c["start"] < c["end"] for c in cues), "Invalid mouth cues"
    duration, fps, size = job["duration_ms"]/1000, job["fps"], job.get("size", 512)
    assert 0 < duration <= 3600 and 1 <= fps <= 30 and 128 <= size <= 768
    output = Path(job["output"])
    temp = output.with_suffix(".pending.mkv")
    width, height = frame_dimensions(job.get("character", "studio"), size)
    command = [job["ffmpeg"], "-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "rgba",
               "-s", f"{width}x{height}", "-r", str(fps), "-i", "pipe:0", "-an", "-t", str(duration),
               "-c:v", "ffv1", "-pix_fmt", "bgra", str(temp)]
    # Inherit stderr so the Rust process runner can drain it and kill the whole process tree.
    proc = subprocess.Popen(command, stdin=subprocess.PIPE, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    try:
        kind = emotion_kind(job.get("emotion", ""))
        gestures = plan_gestures(job.get("narration", ""), job.get("words", []), cues,
                                 kind, job.get("gesture", "auto"), duration)
        for index in range(math.ceil(duration*fps)):
            proc.stdin.write(frame(index/fps, cues, kind, job.get("character", "studio"), size, gestures).tobytes())
        proc.stdin.close()
        if proc.wait() != 0:
            raise RuntimeError("FFmpeg cartoon encode failed")
        temp.replace(output)
    except BaseException:
        proc.kill(); proc.wait()
        raise


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--job", required=True)
    args = parser.parse_args()
    render(json.loads(Path(args.job).read_text(encoding="utf-8")))
