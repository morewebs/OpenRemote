# Archive

## snappy.js — custom wheel-scroll animation (2026-10-01)

Took over wheel ticks with a short exponential settle (~τ12ms) to cut
perceived scroll latency on 144hz. Killed by A/B measurement against
Chromium's native wheel animation, via `scripts/measure-latency.mjs`
(CDP, real wheel events, live window):

| pattern            | native first / settle | snappy first / settle |
| ------------------ | -------------------- | --------------------- |
| 1 tick             | 18.7 / 18.7 ms       | 16.4 / 65 ms          |
| 3 ticks @ 60ms     | 16.2 / 182.8 ms      | 17.7 / 247.1 ms       |

Native wins: single ticks land in ~19ms (no glide worth fighting),
and preventDefault forces the scroll through the main thread, which
costs the first-response frame and lengthens settle. Lesson: the
compositor's native wheel animation is already near-optimal; the
original "latency" feel was the backdrop blur (removed earlier), not
the animation.

Resurrect only with a compositor-side approach; JS takeover cannot
beat it. Measure with `node scripts/measure-latency.mjs` (pass
`native` to disable any active takeover via `window.__snappyOff`).
