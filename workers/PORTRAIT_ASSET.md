# Reference portrait asset

Input: user-supplied reference portrait. Original image and personal filesystem paths are not distributed.
Prepared with built-in image_gen, transparent_background=true, preserving character identity. Output copied to `workers/portrait-base.png`; original retained in the generated-images folder. The preview `cartoon-portrait.png` is one rest frame of the application's native animation worker.

Prompt: Edit the attached character into a transparent head-and-shoulders portrait. Preserve messy dark brown hair, white face, blue-gray edge shading, thick black contours, half-lidded eyes, asymmetric eyebrows and gray hoodie. Entire hair visible; no arms, hands, legs, text, accessories or background. Remove the static mouth to leave white skin so the app can draw animated visemes.

The base is intentionally mouthless. The runtime supplies closed-smile and A-H/X mouth shapes, head movement and transparent compositing. Its fixed mouth anchor is calibrated for this prepared asset; arbitrary uploads require a separate rig setup.

## Half-body rig revision

The renderer isolates and scales the original head to 66% of the previous
logical size. A new original procedural hoodie torso extends to the waist;
the old short collar/body is masked out. Shoulder joints are at (174,279)
and (338,279), separate from the neck at x=236–276. Sleeves are layered
behind the torso shoulder panels, with hands in front. Upper arms are 88px,
forearms 82px and hands approximately 67px wrist-to-fingertip against a
~230px head width. Joint angles, including wrist orientation, interpolate
deterministically: bone lengths never stretch. The palm rotates inward
during preparation/recovery to retain the fingertips within the alpha canvas.

This revision is code-native artwork in cartoon_render.py/gesture_rig.py;
no additional image generation service or motion model is required. Mouth
positions are still drawn in the original head coordinates before scaling.
