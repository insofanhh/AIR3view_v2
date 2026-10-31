"""Check joint geometry across a complete gesture, including transition frames."""
import argparse
import math
import sys
from pathlib import Path


class DrawingBounds:
    """Collect the actual hand drawing primitives, including stroke widths."""
    def __init__(self):
        self.points = []

    def line(self, points, width=1, **_):
        for x, y in points:
            self.points.extend([(x-width/2, y-width/2), (x+width/2, y+width/2)])

    def polygon(self, points, **_):
        self.points.extend(points)

    def ellipse(self, box, **_):
        self.points.extend([(box[0], box[1]), (box[2], box[3])])


def verify(runtime, output):
    sys.path.insert(0, str(runtime / "workers"))
    from PIL import Image, ImageDraw
    from gesture_rig import FOREARM, UPPER_ARM, _hand, arm_pose
    from cartoon_render import frame, frame_dimensions

    kinds = ["none", "open_palms", "point", "beat", "surprised"]
    samples = 0
    for kind in kinds:
        events = [{"start": 0., "end": 2., "kind": kind}]
        for index in range(401):
            for side in [-1, 1]:
                shoulder, elbow, wrist, palm, pose, amount = arm_pose(index/200, events, side)
                assert abs(math.dist(shoulder, elbow)-UPPER_ARM) < 1e-6
                assert abs(math.dist(elbow, wrist)-FOREARM) < 1e-6
                # Fingers must extend beyond the cuff in the forearm direction.
                fingers = (side*math.sin(math.radians(palm)), -math.cos(math.radians(palm)))
                forearm = ((wrist[0]-elbow[0])/FOREARM, (wrist[1]-elbow[1])/FOREARM)
                alignment = sum(a*b for a, b in zip(fingers, forearm))
                assert alignment >= math.cos(math.radians(19)), (kind, index, side, alignment)
                bounds = DrawingBounds()
                _hand(bounds, wrist, palm, side, pose, amount, "#FCFCFD", "#13171E")
                assert all(1 <= x <= 511 and 1 <= y <= 511 for x, y in bounds.points), (kind, index, side)
                samples += 1
        if kind in {"none", "open_palms", "surprised"}:
            for t in [0., .25, .5, .9, 1.4, 1.8, 2.]:
                left, right = arm_pose(t, events, -1), arm_pose(t, events, 1)
                for a, b in zip(left[:3], right[:3]):
                    assert abs(a[0]+b[0]-512) < 1e-6 and abs(a[1]-b[1]) < 1e-6

    cues = [{"start": 0., "end": 2., "value": "D"}]
    for character in ["portrait", "studio", "analyst", "panda"]:
        events = [{"start": 0., "end": 2., "kind": "open_palms"}]
        a = frame(.35, cues, character=character, gestures=events).tobytes()
        b = frame(.7, cues, character=character, gestures=events).tobytes()
        assert a != b, "Gesture must move"
        assert frame(.35, cues, character=character, gestures=events).tobytes() == a, "Seeking must be deterministic"

    # Full-body panda must retain both feet and the robe in every gesture,
    # without losing hat/fingers against any edge of the taller alpha canvas.
    for kind in kinds:
        events = [{"start": 0., "end": 2., "kind": kind}]
        for index in range(41):
            image = frame(index/20, cues, character="panda", gestures=events)
            assert image.size == (512, 704)
            alpha = image.getchannel("A").point(lambda a: 255 if a >= 16 else 0)
            left, top, right, bottom = alpha.getbbox()
            assert 1 <= left < right <= 511 and 1 <= top < bottom <= 703, (kind, index)
            for box in [(194, 654, 240, 682), (272, 654, 318, 682), (217, 550, 295, 630)]:
                assert alpha.crop(box).getextrema()[1] == 255, "Feet and inner robe must remain visible"
    for size in [128, 256, 512, 768]:
        assert frame(0, [], character="panda", size=size).size == frame_dimensions("panda", size)

    if output:
        output.mkdir(parents=True, exist_ok=True)
        times = [0., .175, .35, .58, .79, .95, 1.]
        for character in ["portrait", "panda"]:
            row_height = frame_dimensions(character, 256)[1]+26
            sheet = Image.new("RGB", (len(times)*256, len(kinds)*row_height), "#F1F5F9")
            draw = ImageDraw.Draw(sheet)
            for row, kind in enumerate(kinds):
                events = [{"start": 0., "end": 2., "kind": kind}]
                for col, phase in enumerate(times):
                    image = frame(phase*2, cues, character=character, size=256, gestures=events)
                    sheet.paste(image, (col*256, row*row_height+24), image)
                    draw.text((col*256+8, row*row_height+6), f"{kind} | {phase:.3f}", fill="#172235")
            sheet.save(output / f"{character}-gesture-contact-sheet.png")
    print(f"Avatar rig PASS: {samples} hand poses; fixed bones, wrist alignment, full canvas, mirroring, deterministic motion in all 4 characters; full-body panda with visible robe and feet.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    verify(args.runtime, args.output_dir)
