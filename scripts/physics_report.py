#!/usr/bin/env python3
"""Physics checks for every animal of the aquarium.

Record a run first (headless, fixed 1/60 s step, scripted cursor + clicks,
everybody startled every 20 s):

    AQ_PHYSICS=out/physics AQ_AUTOPILOT=1 AQ_RES=320x208 \
        ./target/release/aquarium --screenshot out/physics/last.png --frames 7200
    python3 scripts/physics_report.py out/physics

`AQ_PHYSICS_DT=30|vsync` records with a 1/30 s step, or with the irregular
steps of a real window that misses refreshes (1/60, 1/30 and a few 0.1 s
hitches); every rate below uses the recorded time, not a frame count.

Exit code 1 if a check fails.
"""

import csv
import math
import sys
from collections import defaultdict

HALF_W, HALF_D, WATER_Y = 0.70, 0.30, 0.645
SPECIES = {
    # name, max speed (m/s), max acceleration (m/s²), turn rate (rad/s)
    "0": ("neon tetra", 0.25, 0.6, 3.2),
    "1": ("angelfish", 0.12, 0.2, 1.2),
    "2": ("clownfish", 0.14, 0.4, 2.4),
    "3": ("discus", 0.10, 0.18, 1.0),
}

results = []


def check(group, name, ok, value, rule, warn=False):
    status = "OK " if ok else ("WARN" if warn else "FAIL")
    results.append((group, name, status, value, rule))


def pct(values, q):
    if not values:
        return float("nan")
    s = sorted(values)
    return s[min(len(s) - 1, int(q * (len(s) - 1)))]


def load(path):
    with open(path) as f:
        return list(csv.DictReader(f))


def vec(r, *keys):
    return tuple(float(r[k]) for k in keys)


def dist(a, b):
    return math.sqrt(sum((x - y) ** 2 for x, y in zip(a, b)))


def wrap(a):
    return (a + math.pi) % (2 * math.pi) - math.pi


def settled(rows):
    """Rows after the first 3 s (placement on the decor happens behind the
    start-up curtain)."""
    t0 = float(rows[0]["t"]) if rows else 0.0
    return [r for r in rows if float(r["t"]) >= t0 + 3.0]


def main(d):
    events = load(f"{d}/events.csv")
    end = max(float(e["t"]) for e in events) if events else 0.0
    last_t = float(load(f"{d}/crab.csv")[-1]["t"]) if load(f"{d}/crab.csv") else end
    # Only the startles with 15 s of recording after them (hideout deadline).
    startles = [float(e["t"]) for e in events if e["event"] == "startle" and float(e["t"]) + 15.0 <= last_t]
    clicks = [float(e["t"]) for e in events if e["event"] == "click"]

    # ------------------------------------------------------------------ fish
    fish = defaultdict(list)
    for r in load(f"{d}/fish.csv"):
        fish[r["id"]].append(r)
    by_species = defaultdict(list)
    for rows in fish.values():
        by_species[rows[0]["species"]].append(settled(rows))
    for sp, tracks in sorted(by_species.items()):
        name, vmax, amax, turn = SPECIES[sp]
        g = f"Fish — {name} ×{len(tracks)}"
        sdfs, speeds, accels, tele, yaw_jerk, roll_jerk, frames = [], [], [], 0, 0, 0, 0
        out_box, pitch_max, bank_max, calm = 0, 0.0, 0.0, []
        panic_jerks = 0
        for rows in tracks:
            prev_v = None
            yaws, rolls, panics, times = [], [], [], []
            panic_start = None
            last_rise = None
            for i, r in enumerate(rows):
                frames += 1
                p = vec(r, "px", "py", "pz")
                f = vec(r, "fx", "fy", "fz")
                u = vec(r, "ux", "uy", "uz")
                s = float(r["speed"])
                sdfs.append(float(r["sdf"]))
                speeds.append(s)
                if abs(p[0]) > HALF_W or abs(p[2]) > HALF_D or p[1] > WATER_Y:
                    out_box += 1
                v = tuple(s * c for c in f)
                if prev_v is not None:
                    dt = float(r["t"]) - float(rows[i - 1]["t"])
                    accels.append(dist(v, prev_v) / dt)
                    q = vec(rows[i - 1], "px", "py", "pz")
                    step = dist(p, q)
                    allowed = max(s, float(rows[i - 1]["speed"])) * dt + 0.001
                    if step > allowed:
                        tele += 1
                prev_v = v
                yaws.append(math.atan2(f[0], f[2]))
                times.append(float(r["t"]))
                pitch_max = max(pitch_max, abs(f[1]))
                # Bank: up vector against the level "right" axis.
                n = math.hypot(f[0], f[2]) or 1.0
                right = (-f[2] / n, 0.0, f[0] / n)
                bank = math.asin(max(-1.0, min(1.0, u[0] * right[0] + u[2] * right[2])))
                rolls.append(bank)
                bank_max = max(bank_max, abs(bank))
                panic = float(r["panic"])
                panics.append(panic)
                # Calm-down is timed from the last stimulus (panic going up).
                if i > 0 and panic > float(rows[i - 1]["panic"]) + 1e-4:
                    last_rise = float(r["t"])
                if panic >= 0.9 and panic_start is None:
                    panic_start = float(r["t"])
                if panic_start is not None and panic < 0.05:
                    calm.append(float(r["t"]) - (last_rise or panic_start))
                    panic_start = None
            # Angular jerks: a startled fish darts (C-start) on purpose; only
            # calm swimming must be perfectly smooth.
            # Angular acceleration (rad/s²), so the threshold doesn't depend
            # on the frame rate: 1.2 and 0.8 rad/s from one 1/60 s frame to the next.
            for series, thr, counter in ((yaws, 72.0, "y"), (rolls, 48.0, "r")):
                w = [wrap(series[i] - series[i - 1]) / (times[i] - times[i - 1]) for i in range(1, len(series))]
                for i in range(1, len(w)):
                    if abs(w[i] - w[i - 1]) / ((times[i + 1] - times[i - 1]) / 2) > thr:
                        if max(panics[max(0, i - 30):i + 2]) > 0.05:
                            panic_jerks += 1
                        elif counter == "y":
                            yaw_jerk += 1
                        else:
                            roll_jerk += 1
        check(g, "never inside the decor (SDF at the centre)", min(sdfs) >= 0.0,
              f"min {min(sdfs) * 1000:.1f} mm, p1 {pct(sdfs, 0.01) * 1000:.1f} mm", "≥ 0")
        check(g, "confinement (glass, surface)", out_box == 0, f"{out_box} frames outside the tank", "0")
        check(g, "top speed", max(speeds) <= vmax * 1.6 * 1.02,
              f"{max(speeds) * 100:.1f} cm/s", f"≤ {vmax * 1.6 * 100:.0f} cm/s (panic)")
        check(g, "acceleration", max(accels) <= amax * 3.5 * 1.6 * 3,
              f"p99 {pct(accels, 0.99):.2f}, max {max(accels):.2f} m/s²",
              f"≤ {amax * 3.5 * 1.6 * 3:.1f} m/s²")
        check(g, "teleports", tele == 0, f"{tele}", "0")
        check(g, "yaw jerks (calm swimming)", yaw_jerk <= frames * 0.001, f"{yaw_jerk} / {frames} frames",
              "≤ 0.1 %", warn=yaw_jerk <= frames * 0.005)
        check(g, "roll jerks (calm swimming)", roll_jerk <= frames * 0.001, f"{roll_jerk} / {frames} frames",
              "≤ 0.1 %", warn=roll_jerk <= frames * 0.005)
        check(g, "sharp turns while panicking (intended)", True, f"{panic_jerks}", "info")
        check(g, "pitch / bank", pitch_max <= 0.47 and bank_max <= 0.36,
              f"pitch {math.degrees(math.asin(pitch_max)):.0f}°, bank {math.degrees(bank_max):.0f}°", "≤ 28° / 20°")
        if calm:
            check(g, "calm again after the last scare", max(calm) <= 3.0,
                  f"{len(calm)} panics, max {max(calm):.2f} s", "≤ 3 s")

    # ------------------------------------------------------------------ crab
    crab = settled(load(f"{d}/crab.csv"))
    if crab:
        g = "Crab"
        contact = [abs(float(r["contact_sdf"])) for r in crab]
        centre = [float(r["centre_sdf"]) for r in crab]
        tilt = [float(r["uy"]) for r in crab]
        speeds = [float(r["speed"]) for r in crab]
        stretch, feet_sdf, slips, planted_frames = [], [], 0, 0
        for i, r in enumerate(crab):
            for k in range(8):
                stretch.append(float(r[f"f{k}_stretch"]))
                if r[f"f{k}_planted"] == "1":
                    planted_frames += 1
                    feet_sdf.append(abs(float(r[f"f{k}_sdf"])))
                    if i > 0 and crab[i - 1][f"f{k}_planted"] == "1":
                        a = vec(r, f"f{k}_x", f"f{k}_y", f"f{k}_z")
                        b = vec(crab[i - 1], f"f{k}_x", f"f{k}_y", f"f{k}_z")
                        if dist(a, b) > 0.0002:
                            slips += 1
        yaw_rates, steps = [], []
        for i in range(1, len(crab)):
            a = vec(crab[i], "fx", "fz")
            b = vec(crab[i - 1], "fx", "fz")
            dt = float(crab[i]["t"]) - float(crab[i - 1]["t"])
            yaw_rates.append(abs(wrap(math.atan2(a[0], a[1]) - math.atan2(b[0], b[1]))) / dt)
            # Beyond what the top speed allows in that frame.
            steps.append(dist(vec(crab[i], "cx", "cy", "cz"), vec(crab[i - 1], "cx", "cy", "cz")) - 0.09 * dt)
        check(g, "standing on the decor (contact point)", pct(contact, 0.99) < 0.002,
              f"p99 {pct(contact, 0.99) * 1000:.2f} mm, max {max(contact) * 1000:.2f} mm", "p99 < 2 mm")
        check(g, "body out of the rocks", min(centre) > 0.0, f"min {min(centre) * 1000:.1f} mm", "> 0")
        if "shell_rock" in crab[0]:
            shell = [float(r["shell_rock"]) for r in crab]
            knees = [float(r["knee_rock"]) for r in crab]
            inside = sum(1 for v in shell if v > 0.002)
            check(g, "carapace and eyes out of the rocks", max(shell) <= 0.002,
                  f"max {max(shell) * 1000:.1f} mm, {inside} frames > 2 mm", "≤ 2 mm")
            check(g, "knees out of the rocks", pct(knees, 0.99) <= 0.002,
                  f"p99 {pct(knees, 0.99) * 1000:.1f} mm, max {max(knees) * 1000:.1f} mm", "p99 ≤ 2 mm",
                  warn=max(knees) <= 0.005)
            if "claw_rock" in crab[0]:
                claws = [float(r["claw_rock"]) for r in crab]
                check(g, "claws out of the rocks", pct(claws, 0.99) <= 0.002,
                      f"p99 {pct(claws, 0.99) * 1000:.1f} mm, max {max(claws) * 1000:.1f} mm", "p99 ≤ 2 mm",
                      warn=max(claws) <= 0.005)
        check(g, "never on its side", min(tilt) > 0.6, f"normal y min {min(tilt):.2f}", "> 0.6 (53°)")
        check(g, "legs within reach (extension)", pct(stretch, 0.99) <= 1.0,
              f"p99 {pct(stretch, 0.99):.2f}, max {max(stretch):.2f}", "p99 ≤ 1.0", warn=max(stretch) <= 1.2)
        check(g, "feet on the ground", pct(feet_sdf, 0.99) < 0.003,
              f"p99 {pct(feet_sdf, 0.99) * 1000:.2f} mm", "< 3 mm")
        check(g, "slipping feet", slips == 0, f"{slips} / {planted_frames}", "0")
        check(g, "top speed", max(speeds) <= 0.09, f"{max(speeds) * 100:.1f} cm/s", "≤ 9 cm/s")
        check(g, "turn rate", max(yaw_rates) <= 3.5, f"max {max(yaw_rates):.2f} rad/s", "≤ 3.5 rad/s")
        check(g, "teleports", max(steps) < 0.003, f"beyond top speed: {max(steps) * 1000:.1f} mm",
              "< 3 mm/frame")
        states = [(float(r["t"]), r["state"]) for r in crab]
        reactions, hides = [], []
        for s in startles:
            flee = next((t for t, st in states if s <= t <= s + 1.0 and st in ("flee", "hidden")), None)
            if flee is not None:
                reactions.append(flee - s)
                hidden = next((t for t, st in states if flee <= t <= flee + 15.0 and st == "hidden"), None)
                if hidden is not None:
                    hides.append(hidden - s)
        if startles:
            check(g, "reacts to a scare", len(reactions) == len(startles),
                  f"{len(reactions)}/{len(startles)}, max delay {max(reactions or [0]):.2f} s", "all, ≤ 1 s")
            check(g, "reaches its hideout", len(hides) == len(reactions),
                  f"{len(hides)}/{len(reactions)}, in {max(hides or [0]):.1f} s max", "≤ 15 s")

    # ---------------------------------------------------- flatfish, starfish
    bottom = defaultdict(list)
    for r in load(f"{d}/bottom.csv"):
        bottom[(r["kind"], r["id"])].append(r)
    for (kind, eid), rows in sorted(bottom.items()):
        rows = settled(rows)
        contact = [abs(float(r["contact_sdf"])) for r in rows]
        tilt = [float(r["uy"]) for r in rows]
        speeds = [float(r["speed"]) for r in rows]
        vmax = 0.17 if kind == "flatfish" else 0.005
        # Beyond what the top speed allows in that frame.
        steps = [dist(vec(rows[i], "px", "py", "pz"), vec(rows[i - 1], "px", "py", "pz"))
                 - vmax * (float(rows[i]["t"]) - float(rows[i - 1]["t"])) for i in range(1, len(rows))]
        if kind == "flatfish":
            g = f"Flatfish #{eid}"
            buried = [float(r["buried"]) for r in rows]
            check(g, "resting on the sand", pct(contact, 0.99) < 0.003,
                  f"p99 {pct(contact, 0.99) * 1000:.2f} mm", "< 3 mm")
            check(g, "stays flat", min(tilt) > 0.9, f"normal y min {min(tilt):.3f}", "> 0.9")
            check(g, "top speed", max(speeds) <= 0.17, f"{max(speeds) * 100:.1f} cm/s", "≤ 17 cm/s")
            check(g, "teleports", max(steps) < 0.003, f"beyond top speed: {max(steps) * 1000:.1f} mm",
                  "< 3 mm/frame")
            check(g, "buries itself at rest", max(buried) >= 0.6 and min(buried) >= 0.0,
                  f"burial {min(buried):.2f}…{max(buried):.2f}", "reaches ≥ 0.6")
            modes = [(float(r["t"]), r["mode"]) for r in rows]
            reacted = sum(1 for s in startles if any(s <= t <= s + 0.5 and m == "startle" for t, m in modes))
            if startles:
                check(g, "reacts to a scare", reacted == len(startles), f"{reacted}/{len(startles)}", "all, ≤ 0.5 s")
        else:
            glass = abs(float(rows[0]["pz"]) - HALF_D) < 0.01
            g = f"Starfish #{eid} ({'glass' if glass else 'rock'})"
            check(g, "stuck to its surface", pct(contact, 0.99) < 0.002,
                  f"p99 {pct(contact, 0.99) * 1000:.2f} mm", "< 2 mm")
            check(g, "slowness", max(speeds) <= 0.005, f"max {max(speeds) * 1000:.1f} mm/s", "≤ 5 mm/s")
            check(g, "teleports", max(steps) < 0.001, f"beyond top speed: {max(steps) * 1000:.2f} mm",
                  "< 1 mm/frame")
            if glass:
                off = max(abs(float(r["pz"]) - HALF_D) for r in rows)
                check(g, "stays on the glass", off < 0.004, f"max offset {off * 1000:.1f} mm", "< 4 mm")

    # ---------------------------------------------------------------- flakes
    flakes = load(f"{d}/flakes.csv")
    if flakes:
        g = "Food (flakes)"
        sink = [float(r["vy"]) for r in flakes if r["state"] == "sinking"]
        inside = [float(r["sdf"]) for r in flakes]
        resting = [float(r["sdf"]) for r in flakes if r["state"] == "resting"]
        floating = [float(r["py"]) for r in flakes if r["state"] == "floating"]
        check(g, "slow sinking (vertical speed)", min(sink) >= -0.03 and max(sink) <= 0.001,
              f"{min(sink) * 100:.1f} … {max(sink) * 100:.1f} cm/s", "between -3 and 0 cm/s")
        check(g, "never inside the decor", min(inside) >= -0.001, f"min {min(inside) * 1000:.2f} mm", "≥ -1 mm")
        if resting:
            check(g, "lying flat on the bottom", max(abs(v - 0.0012) for v in resting) < 0.002,
                  f"max offset {max(abs(v - 0.0012) for v in resting) * 1000:.2f} mm", "< 2 mm")
        if floating:
            check(g, "floating on the surface", max(abs(y - (WATER_Y - 0.0012)) for y in floating) < 0.0015,
                  f"max offset {max(abs(y - (WATER_Y - 0.0012)) for y in floating) * 1000:.2f} mm", "< 1.5 mm")
        # Fate of each flake: its last state before it leaves the water.
        per_frame = defaultdict(list)
        for r in flakes:
            per_frame[int(r["frame"])].append(r)
        gone, states, prev_ids = defaultdict(int), {}, set()
        for fr in sorted(per_frame):
            ids = {r["id"] for r in per_frame[fr]}
            for i in prev_ids - ids:
                gone[states[i]] += 1
            for r in per_frame[fr]:
                states[r["id"]] = r["state"]
            prev_ids = ids
        eaten_fish = gone["floating"] + gone["sinking"]
        check(g, "eaten by the fish", eaten_fish > 0,
              f"{eaten_fish} in open water, {gone['resting']} on the bottom, {gone['fading']} dissolved", "> 0")
        # Per click: the flakes that appeared right after it, and when the first
        # of them was eaten.
        appear, eaten_at, seen = {}, {}, set()
        prev_ids = set()
        for fr in sorted(per_frame):
            ids = {r["id"] for r in per_frame[fr]}
            for i in ids - prev_ids:
                appear.setdefault(i, []).append(fr)
            for i in prev_ids - ids:
                eaten_at.setdefault(i, []).append(fr)
            prev_ids = ids
        frame_t = {int(r["frame"]): float(r["t"]) for r in flakes}
        lat = []
        for c in clicks:
            eats = []
            for i, apps in appear.items():
                for a in apps:
                    if c <= frame_t[a] <= c + 1.5:
                        after = [e for e in eaten_at.get(i, []) if e > a]
                        if after:
                            eats.append(after[0])
            if eats:
                lat.append(frame_t[min(eats)] - c)
        if lat:
            check(g, "the fish come", pct(lat, 0.5) <= 8.0,
                  f"1st flake eaten {min(lat):.1f}–{max(lat):.1f} s after the click (median {pct(lat, 0.5):.1f} s)",
                  "median ≤ 8 s")

    # --------------------------------------------------------------- bubbles
    bubbles = load(f"{d}/bubbles.csv")
    if bubbles:
        g = "Bubbles"
        vy = [float(r["vy"]) for r in bubbles]
        ys = [float(r["py"]) for r in bubbles]
        check(g, "rise", min(vy) > 0.0, f"{min(vy) * 100:.1f} … {max(vy) * 100:.1f} cm/s", "> 0")
        check(g, "realistic terminal speed", max(vy) <= 0.25, f"max {max(vy) * 100:.1f} cm/s", "≤ 25 cm/s")
        check(g, "vanish at the surface", max(ys) <= WATER_Y, f"y max {max(ys):.3f} m", f"≤ {WATER_Y}")

    # ---------------------------------------------------------------- report
    width = max(len(n) for _, n, *_ in results)
    group = None
    fails = 0
    for grp, n, status, value, rule in results:
        if grp != group:
            print(f"\n{grp}")
            group = grp
        fails += status == "FAIL"
        print(f"  [{status:4}] {n:<{width}}  {value}   (limit {rule})")
    total = len(results)
    warns = sum(1 for r in results if r[2] == "WARN")
    print(f"\n{total - fails - warns} OK, {warns} to watch, {fails} failed, out of {total} checks")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "out/physics"))
