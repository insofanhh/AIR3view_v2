"""Small seek-safe presentation rig. Original artwork and authored motion curves.

Reference principles (not copied motion data): Toastmasters gestures/body language,
and Gesticulator (ICMI 2020): match semantic intent and speech rhythm, allow rests.
"""
import math
import re


def plan_gestures(text, words, mouth_cues, style, duration):
    if style == "none":
        return []
    speech = [c for c in mouth_cues if c["value"] != "X"]
    if not speech:
        return []
    tokens = text.split()
    if not words and tokens:
        start, end = speech[0]["start"], min(duration, speech[-1]["end"])
        words = [{"text": token, "start_ms": (start + (end-start)*i/len(tokens))*1000}
                 for i, token in enumerate(tokens)]
    events = []
    next_time = 0.0
    for word in words:
        t = float(word["start_ms"])/1000
        if t < next_time or t > duration-.4:
            continue
        # Wait for the next actual speech onset; ASR pauses are not gestures.
        voiced = next((c for c in speech if c["end"] > t and c["start"] < t+.6), None)
        if voiced is None:
            continue
        t = max(t, voiced["start"])
        token = re.sub(r"[^\w]", "", word["text"].lower())
        kind = style
        if kind == "auto":
            if token in {"nhìn", "xem", "đây", "kìa", "watch", "look"}:
                kind = "point"
            elif token in {"sao", "nào", "vì", "nhưng", "nghĩa", "why", "how"}:
                kind = "open_palms"
            else:
                kind = "beat" if len(events) % 2 else "open_palms"
        start = max(0, t-.25)
        events.append({"start": start, "end": min(duration, t+1.35), "kind": kind})
        next_time = t + (3.2 if kind != "beat" else 2.6)
    return events


def gesture_pose(t, events):
    """Preparation -> stroke -> hold -> recovery, with a real neutral rest."""
    for event in events:
        if event["start"] <= t < event["end"]:
            u = (t-event["start"])/(event["end"]-event["start"])
            if u < .35:
                v = u/.35
                amount = v*v*(3-2*v)
            elif u < .58:
                amount = 1.0
            else:
                v = (u-.58)/.42
                amount = 1-v*v*(3-2*v)
            return event["kind"], amount
    return "none", 0.0


UPPER_ARM, FOREARM = 88.0, 82.0
SHOULDERS = {-1: (174.0, 279.0), 1: (338.0, 279.0)}


def arm_pose(t, events, side):
    """Forward kinematics: fixed bone lengths and a shoulder on the torso.

    Angles are interpolated, rather than independent elbow/wrist positions, so
    limbs cannot stretch or switch elbow solutions during preparation/recovery.
    The wrist flex is relative to the forearm. Palm orientation is derived only
    after interpolating the arm, so fingers never rotate back toward the elbow.
    """
    kind, amount = gesture_pose(t, events)
    rest = (24, -28, 0)
    target = {
        "none": rest,
        "open_palms": (25, 155, -12),
        "point": (10, 150, 18) if side == -1 else rest,
        "beat": (16, 150, -8) if side == 1 else rest,
        "surprised": (42, 176, -4),
    }[kind]
    upper, lower, flex = [a+(b-a)*amount for a, b in zip(rest, target)]
    # Tuck the elbow during the sweep instead of folding the hand inward just
    # to fit the canvas. Rest/full-extension poses retain their authored angles.
    if target != rest:
        # First lift the elbow outward so a downward-facing hand clears the
        # waist, then tuck it as the forearm passes through horizontal.
        upper += 18 * math.sin(2 * math.pi * amount) - 18 * math.sin(math.pi * amount)
    palm = 180 - lower + flex
    shoulder = SHOULDERS[side]
    def end(start, angle, length):
        angle = math.radians(angle)
        return (start[0]+side*math.sin(angle)*length, start[1]+math.cos(angle)*length)
    elbow = end(shoulder, upper, UPPER_ARM)
    wrist = end(elbow, lower, FOREARM)
    return shoulder, elbow, wrist, palm, kind, amount


def _capsule(draw, a, b, r1, r2, color, ink):
    dx, dy = b[0]-a[0], b[1]-a[1]
    length = math.hypot(dx, dy)
    nx, ny = -dy/length, dx/length
    points = [(a[0]+nx*r1, a[1]+ny*r1), (b[0]+nx*r2, b[1]+ny*r2),
              (b[0]-nx*r2, b[1]-ny*r2), (a[0]-nx*r1, a[1]-ny*r1)]
    draw.polygon(points, fill=color)
    for p, radius in [(a, r1), (b, r2)]:
        draw.ellipse((p[0]-radius, p[1]-radius, p[0]+radius, p[1]+radius), fill=color)
    draw.line([points[0], points[1]], fill=ink, width=3)
    draw.line([points[2], points[3]], fill=ink, width=3)


def _hand(draw, wrist, angle, side, kind, amount, skin, ink):
    # A 38 px palm, ~67 px wrist-to-fingertip, against a 230 px head.
    # All fingers and the thumb follow the wrist rotation, including in rest.
    angle = math.radians(angle)*side
    def p(x, y):
        return (wrist[0]+side*x*math.cos(angle)-y*math.sin(angle),
                wrist[1]+side*x*math.sin(angle)+y*math.cos(angle))
    def stroke(points, width, color):
        points = [p(x, y) for x, y in points]
        draw.line(points, fill=color, width=width, joint="curve")
        radius = width/2
        for x, y in [points[0], points[-1]]:
            draw.ellipse((x-radius, y-radius, x+radius, y+radius), fill=color)
    # Fingers first, then palm: avoids seam lines crossing the finger roots.
    pointing = kind == "point" and side == -1
    for x, length in [(-13, 22), (-4, 30), (5, 27), (14, 19)]:
        fold = (1-amount) if pointing and x != -13 else 1
        stroke([(x, -26), (x, -35-length*fold)], 10, ink)
        stroke([(x, -26), (x, -35-length*fold)], 5, skin)
    outline = [p(-18, 2), p(-20, -22), p(-17, -36), p(18, -36), p(20, -20), p(13, 3)]
    draw.polygon(outline, fill=skin)
    draw.line(outline+[outline[0]], fill=ink, width=3, joint="curve")
    stroke([(-13, -5), (-30, -23), (-29, -31)], 12, ink)
    stroke([(-13, -5), (-30, -23), (-29, -31)], 7, skin)
    stroke([(-6, -18), (6, -23)], 2, "#AFBDCB" if skin == "#FCFCFD" else "#CE8D69")


def draw_arms(draw, t, events, portrait=False, shirt="#536EF6", layer="all", outfit="hoodie"):
    ink, skin = ("#13171E", "#FCFCFD") if portrait else ("#172235", "#F1B78F")
    shirt = "#E2E8F0" if portrait else shirt
    robe = outfit == "robe"
    if robe:
        ink, skin, shirt = "#242632", "#FCFCFD", "#9699BA"
    for side in [-1, 1]:
        shoulder, elbow, wrist, palm, kind, amount = arm_pose(t, events, side)
        if layer in {"all", "sleeves"}:
            # Sleeves emerge underneath the shoulder panel, not from the hood.
            _capsule(draw, shoulder, elbow, 27, 22, ink, ink)
            _capsule(draw, elbow, wrist, 22, 25 if robe else 17, ink, ink)
            _capsule(draw, shoulder, elbow, 24, 19, shirt, shirt)
            _capsule(draw, elbow, wrist, 19, 22 if robe else 14, shirt, shirt)
            # Elbow fold and ribbed cuff make the bend/readable wrist explicit.
            dx, dy = wrist[0]-elbow[0], wrist[1]-elbow[1]
            nx, ny = -dy/FOREARM, dx/FOREARM
            if robe:
                draw.line([(elbow[0]+nx*9, elbow[1]+ny*9),
                           (wrist[0]+nx*15-dx*.1, wrist[1]+ny*15-dy*.1)], fill="#646786", width=3)
                draw.line([(wrist[0]-nx*22, wrist[1]-ny*22),
                           (wrist[0]+nx*22, wrist[1]+ny*22)], fill=ink, width=3)
            else:
                for distance in [3, 8]:
                    x, y = wrist[0]-dx/FOREARM*distance, wrist[1]-dy/FOREARM*distance
                    draw.line([(x-nx*13, y-ny*13), (x+nx*13, y+ny*13)], fill="#9BAEC4" if portrait else "#364A9F", width=2)
        if layer in {"all", "hands"}:
            _hand(draw, wrist, palm, side, kind, amount, skin, ink)


def draw_robe_body(draw, t, events):
    """Full lavender robe on a 512x704 canvas, with layered skirt and feet."""
    ink, coat, shade = "#242632", "#9699BA", "#646786"
    sway = math.sin(t*2.4)*3
    # Feet/trousers sit behind the hem. Keep a clear gap under the garment,
    # including at the extremes of the deterministic cloth sway.
    for side in [-1, 1]:
        x = 256+side*39
        draw.polygon([(x-17, 607), (x+17, 607), (x+19, 665), (x-18, 665)],
                     fill="#EFF1ED", outline=ink, width=3)
        draw.rounded_rectangle((x-23, 654, x+24, 681), 12, fill="#282A31", outline=ink, width=3)
        draw.line([(x-17, 674), (x+18, 674)], fill="#555965", width=2)
    draw_arms(draw, t, events, layer="sleeves", outfit="robe")
    contour = [(230, 243), (207, 250), (176, 266), (173, 303), (181, 362),
               (169, 474), (145+sway, 619), (184+sway, 645), (217+sway, 626),
               (295+sway, 626), (328+sway, 645), (367+sway, 619), (343, 474), (331, 362),
               (339, 303), (336, 266), (305, 250), (282, 243)]
    draw.polygon(contour, fill=coat)
    draw.line(contour+[contour[0]], fill=ink, width=4, joint="curve")
    draw.polygon([(185, 319), (201, 348), (193, 458), (174+sway, 604),
                  (159+sway, 617), (180, 462)], fill=shade)
    draw.polygon([(327, 319), (311, 348), (319, 458), (338+sway, 604),
                  (353+sway, 617), (332, 462)], fill=shade)
    # Continuous white under-robe, visible between two open lavender panels.
    inner = [(231, 248), (281, 248), (291, 411), (314+sway, 645),
             (279+sway, 653), (236+sway, 653), (198+sway, 645), (221, 411)]
    draw.polygon(inner, fill="#F9FAF7", outline=ink, width=3)
    draw.line([(231, 248), (277, 362), (276, 412)], fill=ink, width=3)
    draw.line([(281, 248), (246, 337), (237, 412)], fill=ink, width=3)
    # Long pleats and open coat edges read as cloth instead of a flat block.
    draw.polygon([(238, 413), (227+sway, 642), (211+sway, 645)], fill="#D5DAD8")
    draw.polygon([(279, 414), (300+sway, 643), (281+sway, 648)], fill="#DDE1DE")
    draw.line([(253, 429), (249+sway, 636)], fill="#A8B0AE", width=2)
    for side in [-1, 1]:
        draw.line([(256+side*47, 410), (256+side*49, 493),
                   (256+side*58+sway, 629)], fill=ink, width=3, joint="curve")
        draw.line([(256+side*67, 458), (256+side*80+sway, 601)], fill="#BCBED4", width=3)
    draw.polygon([(222, 251), (202, 279), (218, 294), (208, 310), (234, 373), (245, 358)], fill="#B5B7D0")
    draw.polygon([(290, 251), (310, 279), (294, 294), (304, 310), (278, 373), (267, 358)], fill="#B5B7D0")
    draw.line([(222, 251), (202, 279), (218, 294), (208, 310), (234, 373)], fill=ink, width=3)
    draw.line([(290, 251), (310, 279), (294, 294), (304, 310), (278, 373)], fill=ink, width=3)
    draw.rounded_rectangle((209, 371, 303, 410), 3, fill="#38343F", outline=ink, width=3)
    draw.line([(211, 383), (301, 383)], fill="#615966", width=2)
    draw.line([(211, 398), (301, 398)], fill="#615966", width=2)
    swing = math.sin(t*2.4)*3
    draw.line([(228, 410), (228+swing, 467)], fill=ink, width=2)
    for y, fill in [(419, "#DCE9E4"), (431, "#F8FAF6"), (443, "#B7C9C2")]:
        x = 228+swing*(y-410)/41
        draw.ellipse((x-5, y-5, x+5, y+5), fill=fill, outline=ink, width=2)
    x = 228+swing
    draw.ellipse((x-4, 453, x+4, 464), fill="#B73D47", outline=ink, width=1)
    draw.polygon([(x-4, 465), (x+4, 465), (x+7, 517), (x, 525), (x-7, 517)], fill="#D3AE55", outline=ink)
    for dx in [-3, 0, 3]:
        draw.line([(x+dx, 470), (x+dx*1.6, 517)], fill="#94733B", width=1)
    draw_arms(draw, t, events, layer="hands", outfit="robe")


def draw_half_body(draw, t, events, portrait=False, shirt="#536EF6"):
    """Waist-up hoodie with separate shoulder caps, torso and foreground hands."""
    ink = "#13171E" if portrait else "#172235"
    shirt = "#E2E8F0" if portrait else shirt
    shade = "#AFBDCE" if portrait else "#364A9F"
    draw_arms(draw, t, events, portrait, shirt, layer="sleeves")
    # The neck/collar is 66 px wide; shoulder joints sit 82 px from center.
    # Continuous outline includes both shoulder slopes and the waist.
    contour = [(220, 244), (202, 250), (182, 258), (170, 270), (170, 306),
               (178, 361), (172, 482), (181, 492), (331, 492), (340, 482),
               (334, 361), (342, 306), (342, 270), (330, 258), (310, 250), (292, 244)]
    # Round the outline while retaining a continuous shoulder-to-waist panel.
    for _ in range(2):
        contour = [point for a, b in zip(contour, contour[1:]+contour[:1])
                   for point in [(a[0]*.75+b[0]*.25, a[1]*.75+b[1]*.25),
                                 (a[0]*.25+b[0]*.75, a[1]*.25+b[1]*.75)]]
    draw.polygon(contour, fill=shirt)
    draw.line(contour+[contour[0]], fill=ink, width=4, joint="curve")
    draw.polygon([(179, 312), (192, 340), (193, 476), (177, 481)], fill=shade)
    draw.polygon([(333, 312), (320, 340), (319, 476), (335, 481)], fill=shade)
    # Shoulder seams are lateral and below the hood.
    for side in [-1, 1]:
        draw.line([(256+side*68, 268), (256+side*78, 299), (256+side*76, 320)], fill=shade, width=3)
    draw.ellipse((203, 227, 309, 285), fill=shade, outline=ink, width=4)
    draw.ellipse((221, 229, 291, 268), fill="#26313C", outline=ink, width=3)
    draw.rounded_rectangle((236, 214, 276, 255), 10, fill="#FCFCFD" if portrait else "#F1B78F", outline=ink, width=3)
    draw.line([(208, 250), (222, 270), (256, 292), (290, 270), (304, 250)], fill=ink, width=4, joint="curve")
    for x in [231, 281]:
        draw.line([(x, 279), (x, 334)], fill=ink, width=7)
        draw.line([(x, 279), (x, 333)], fill="#F8FAFC", width=3)
    # Kangaroo pocket and hem make the half-body silhouette read as a torso.
    draw.line([(217, 399), (200, 427), (202, 457), (310, 457), (312, 427), (295, 399)], fill=shade, width=3, joint="curve")
    draw.line([(181, 480), (331, 480)], fill=shade, width=3)
    draw_arms(draw, t, events, portrait, shirt, layer="hands")
