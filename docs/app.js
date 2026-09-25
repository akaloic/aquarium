// Aquarium — project page. Vanilla JS, no build step.
"use strict";

const $ = (s, r = document) => r.querySelector(s);
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const smoothstep = (a, b, x) => { const t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); };
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Runs `step(dt)` every frame while `el` is on screen. */
function animateWhileVisible(el, step) {
  let running = false, last = 0, raf = 0;
  const loop = (t) => {
    const dt = Math.min(0.05, (t - last) / 1000 || 0.016);
    last = t;
    step(dt, t / 1000);
    if (running) raf = requestAnimationFrame(loop);
  };
  new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting && !running) { running = true; last = performance.now(); raf = requestAnimationFrame(loop); }
      else if (!e.isIntersecting && running) { running = false; cancelAnimationFrame(raf); }
    }
  }, { threshold: 0.02 }).observe(el);
}

/** Matches a canvas' backing store to its CSS size. */
function fit(canvas) {
  const r = canvas.getBoundingClientRect();
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.round(r.width * dpr)), h = Math.max(1, Math.round(r.height * dpr));
  if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
  return { w: r.width, h: r.height, dpr };
}

// --------------------------------------------------------------- reveal
{
  const els = document.querySelectorAll(".section h2, .section .wide, .demo, .facts, .watts, .bugs, .budget, .clip, .states, .dvfs");
  const io = new IntersectionObserver((es) => es.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } }), { threshold: 0.01, rootMargin: "0px 0px -40px 0px" });
  els.forEach((el) => { el.classList.add("reveal"); io.observe(el); });
}
if (reduced) document.querySelectorAll("video").forEach((v) => { v.removeAttribute("autoplay"); v.pause(); v.controls = true; });

// --------------------------------------------------------------- copy
document.querySelectorAll(".copy").forEach((b) => b.addEventListener("click", async () => {
  const text = $("#" + b.dataset.copy).innerText.split("\n").filter((l) => !l.startsWith("#")).join("\n").trim();
  try { await navigator.clipboard.writeText(text); b.textContent = "Copied"; } catch { b.textContent = "Select + ⌘C"; }
  setTimeout(() => (b.textContent = "Copy"), 1600);
}));

// ============================================================ 1. the school
{
  const canvas = $("#school"), ctx = canvas.getContext("2d"), hud = $("#school-state");
  let W = 0, H = 0, dpr = 1, surf = 0, floor = 0;
  const mouse = { x: -1e3, y: -1e3, in: false, vx: 0, vy: 0 };
  const fish = [], flakes = [], rings = [], motes = [];
  const KINDS = {
    neon: { len: 15, cruise: 62, panic: 200, turn: 2.4 },
    clown: { len: 26, cruise: 38, panic: 130, turn: 1.6 },
  };
  const resize = () => {
    ({ w: W, h: H, dpr } = fit(canvas));
    surf = H * 0.09; floor = H * 0.86;
  };
  resize();
  addEventListener("resize", resize);
  for (let i = 0; i < 40; i++) {
    const kind = i < 36 ? "neon" : "clown";
    fish.push({ kind, x: W * (0.2 + 0.6 * Math.random()), y: surf + (floor - surf) * (0.25 + 0.5 * Math.random()),
      a: Math.random() * 6.28, w: 0, s: KINDS[kind].cruise, panic: 0, phase: Math.random() * 6.28, seed: Math.random() * 100 });
  }
  for (let i = 0; i < 70; i++) motes.push({ x: Math.random(), y: Math.random(), z: 0.3 + Math.random() * 0.7 });

  const pos = (e) => { const r = canvas.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; };
  canvas.addEventListener("pointermove", (e) => {
    const [x, y] = pos(e);
    if (mouse.in) { mouse.vx = x - mouse.x; mouse.vy = y - mouse.y; }
    Object.assign(mouse, { x, y, in: true });
  });
  canvas.addEventListener("pointerleave", () => { mouse.in = false; mouse.x = mouse.y = -1e3; });
  canvas.addEventListener("pointerdown", (e) => {
    const [x] = pos(e);
    for (let i = 0; i < 7; i++) flakes.push({ x: x + (Math.random() - 0.5) * 40, y: surf + 2, vy: 0, age: -0.6 - Math.random() * 0.8, rot: Math.random() * 6, spin: (Math.random() - 0.5) * 4, seed: Math.random() * 10 });
    rings.push({ x, r: 2, a: 1 });
  });

  const angDiff = (a, b) => Math.atan2(Math.sin(a - b), Math.cos(a - b));

  function update(dt, t) {
    // Flakes float a moment, then flutter down.
    for (const f of flakes) {
      f.age += dt;
      if (f.age > 0) { f.vy += (22 - f.vy) * Math.min(1, dt * 2); f.y += f.vy * dt; f.x += Math.sin(t * 2 + f.seed) * 12 * dt; }
      f.rot += f.spin * dt;
      if (f.y > floor - 3) { f.y = floor - 3; f.vy = 0; f.rest = (f.rest || 0) + dt; }
    }
    for (let i = flakes.length - 1; i >= 0; i--) if ((flakes[i].rest || 0) > 6) flakes.splice(i, 1);
    for (const r of rings) { r.r += 60 * dt; r.a -= dt * 0.9; }
    for (let i = rings.length - 1; i >= 0; i--) if (rings[i].a <= 0) rings.splice(i, 1);

    let panicking = 0, feeding = 0;
    for (const f of fish) {
      const k = KINDS[f.kind];
      let fx = 0, fy = 0, ax = 0, ay = 0, cx = 0, cy = 0, n = 0;
      for (const o of fish) {
        if (o === f) continue;
        const dx = o.x - f.x, dy = o.y - f.y, d = Math.hypot(dx, dy);
        if (d < 22 && d > 0) { fx -= (dx / d) * (1 - d / 22) * 2.2; fy -= (dy / d) * (1 - d / 22) * 2.2; }
        if (o.kind === f.kind && d < 80) { ax += Math.cos(o.a); ay += Math.sin(o.a); cx += dx; cy += dy; n++; }
        // Panic spreads through the school.
        if (d < 70 && o.panic > f.panic + 0.1) f.panic = Math.max(f.panic, o.panic * 0.82);
      }
      if (n) { fx += (ax / n) * 0.9 + (cx / n) * 0.012; fy += (ay / n) * 0.9 + (cy / n) * 0.012; }
      // Wander.
      const wa = Math.sin(t * 0.4 + f.seed) * 1.4 + Math.sin(t * 0.13 + f.seed * 3) * 1.8;
      fx += Math.cos(wa) * 0.5; fy += Math.sin(wa) * 0.35;
      // Glass, surface and sand, softly.
      const m = 70;
      if (f.x < m) fx += (m - f.x) / m * 3; if (f.x > W - m) fx -= (f.x - W + m) / m * 3;
      if (f.y < surf + 30) fy += (surf + 30 - f.y) / 30 * 3; if (f.y > floor - 30) fy -= (f.y - floor + 30) / 30 * 3;
      // The cursor finger.
      const mdx = f.x - mouse.x, mdy = f.y - mouse.y, md = Math.hypot(mdx, mdy);
      if (md < 120) { const s = (1 - md / 120) * 7; fx += (mdx / md) * s; fy += (mdy / md) * s; f.panic = Math.max(f.panic, 1 - md / 160); }
      // Food.
      let target = null, best = 260;
      if (f.panic < 0.4) for (const fl of flakes) { const d = Math.hypot(fl.x - f.x, fl.y - f.y); if (d < best) { best = d; target = fl; } }
      if (target) {
        fx += (target.x - f.x) / best * 3; fy += (target.y - f.y) / best * 3; feeding++;
        if (best < 9) flakes.splice(flakes.indexOf(target), 1);
      }
      // Steering with bounded angular acceleration: no frame shows a jerk.
      const want = Math.atan2(fy, fx);
      const maxTurn = k.turn * (1 + f.panic * 2.5), acc = 10 + f.panic * 50;
      const wTarget = clamp(angDiff(want, f.a) * 3, -maxTurn, maxTurn);
      f.w += clamp(wTarget - f.w, -acc * dt, acc * dt);
      f.a += f.w * dt;
      const speed = k.cruise * (target ? 1.4 : 1) * (1 + f.panic * (k.panic / k.cruise - 1));
      f.s += (speed - f.s) * Math.min(1, dt * 3);
      f.x += Math.cos(f.a) * f.s * dt; f.y += Math.sin(f.a) * f.s * dt;
      f.x = clamp(f.x, 8, W - 8); f.y = clamp(f.y, surf + 8, floor - 8);
      f.phase += f.s * dt * 0.22;
      f.panic = Math.max(0, f.panic - dt * 0.45);
      if (f.panic > 0.3) panicking++;
    }
    hud.textContent = panicking > 4 ? "panic — spreading through the school" : feeding ? "feeding" : "calm";
  }

  function drawFish(f) {
    const k = KINDS[f.kind], L = k.len;
    ctx.save();
    ctx.translate(f.x, f.y);
    ctx.rotate(f.a);
    const flip = Math.cos(f.a) < 0 ? -1 : 1;
    ctx.scale(1, flip);
    const wig = Math.sin(f.phase) * 0.35;
    if (f.kind === "neon") {
      ctx.fillStyle = "rgba(210,225,235,0.55)";
      ctx.beginPath(); ctx.ellipse(0, 0, L / 2, L * 0.17, 0, 0, 7); ctx.fill();
      ctx.fillStyle = "#ff4d5a";
      ctx.beginPath(); ctx.ellipse(-L * 0.18, L * 0.04, L * 0.3, L * 0.1, 0, 0, 7); ctx.fill();
      ctx.shadowColor = "#4ff0ff"; ctx.shadowBlur = 6;
      ctx.fillStyle = "#6ff4ff";
      ctx.fillRect(-L * 0.22, -L * 0.07, L * 0.62, L * 0.07);
      ctx.shadowBlur = 0;
      ctx.fillStyle = "rgba(210,225,235,0.5)";
      ctx.beginPath(); ctx.moveTo(-L / 2 + 1, 0); ctx.lineTo(-L * 0.78, -L * 0.18 + wig * 4); ctx.lineTo(-L * 0.78, L * 0.18 + wig * 4); ctx.fill();
    } else {
      ctx.fillStyle = "#ff7a2e";
      ctx.beginPath(); ctx.ellipse(0, 0, L / 2, L * 0.3, 0, 0, 7); ctx.fill();
      ctx.save(); ctx.clip();
      ctx.fillStyle = "#fff";
      for (const x of [0.22, -0.02, -0.3]) ctx.fillRect(x * L, -L, L * 0.09, 2 * L);
      ctx.restore();
      ctx.strokeStyle = "rgba(20,10,5,0.6)"; ctx.lineWidth = 1;
      ctx.beginPath(); ctx.ellipse(0, 0, L / 2, L * 0.3, 0, 0, 7); ctx.stroke();
      ctx.fillStyle = "#ff7a2e";
      ctx.beginPath(); ctx.moveTo(-L / 2 + 2, 0); ctx.lineTo(-L * 0.75, -L * 0.24 + wig * 6); ctx.lineTo(-L * 0.75, L * 0.24 + wig * 6); ctx.fill();
      ctx.fillStyle = "#111"; ctx.beginPath(); ctx.arc(L * 0.3, -L * 0.06, 1.8, 0, 7); ctx.fill();
    }
    ctx.restore();
  }

  function draw(t) {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const g = ctx.createLinearGradient(0, 0, 0, H);
    g.addColorStop(0, "#0d4468"); g.addColorStop(0.55, "#072a47"); g.addColorStop(1, "#03121f");
    ctx.fillStyle = g; ctx.fillRect(0, 0, W, H);
    // Light shafts.
    ctx.globalCompositeOperation = "lighter";
    for (let i = 0; i < 7; i++) {
      const x = W * (i + 0.5) / 7 + Math.sin(t * 0.2 + i) * 30, wdt = 30 + 25 * Math.sin(i * 2.3), a = 0.05 + 0.04 * Math.sin(t * 0.5 + i * 1.7);
      const sg = ctx.createLinearGradient(0, surf, 0, floor);
      sg.addColorStop(0, `rgba(140,220,255,${a})`); sg.addColorStop(1, "rgba(140,220,255,0)");
      ctx.fillStyle = sg;
      ctx.beginPath(); ctx.moveTo(x - wdt / 2, surf); ctx.lineTo(x + wdt / 2, surf); ctx.lineTo(x + wdt / 2 + 90, floor); ctx.lineTo(x - wdt / 2 + 60, floor); ctx.fill();
    }
    ctx.globalCompositeOperation = "source-over";
    // Sand with moving caustics.
    const sg = ctx.createLinearGradient(0, floor, 0, H);
    sg.addColorStop(0, "#8a7a58"); sg.addColorStop(1, "#3a3122");
    ctx.fillStyle = sg; ctx.fillRect(0, floor, W, H - floor);
    ctx.globalCompositeOperation = "lighter";
    for (let i = 0; i < 26; i++) {
      const x = (i * 97.3 + t * 9 * (1 + (i % 3))) % (W + 60) - 30, y = floor + 6 + ((i * 37) % Math.max(8, H - floor - 10));
      ctx.fillStyle = `rgba(255,240,200,${0.06 + 0.05 * Math.sin(t * 1.3 + i)})`;
      ctx.beginPath(); ctx.ellipse(x, y, 22, 4, 0, 0, 7); ctx.fill();
    }
    ctx.globalCompositeOperation = "source-over";
    // Plants on both sides.
    ctx.lineCap = "round";
    for (let i = 0; i < 16; i++) {
      const left = i < 8, bx = left ? 14 + i * 13 : W - 14 - (i - 8) * 13, h = (0.45 + 0.35 * ((i * 7) % 5) / 5) * (floor - surf);
      const sway = Math.sin(t * 0.8 + i * 0.7) * 26;
      ctx.strokeStyle = `rgba(${40 + i * 4},${150 + (i % 4) * 20},${70 + i * 3},0.7)`;
      ctx.lineWidth = 3.5;
      ctx.beginPath(); ctx.moveTo(bx, floor + 4); ctx.quadraticCurveTo(bx + sway * 0.3, floor - h * 0.6, bx + sway, floor - h); ctx.stroke();
    }
    // Plankton.
    ctx.fillStyle = "rgba(200,235,255,0.55)";
    for (const m of motes) {
      const x = ((m.x * W + t * 6 * m.z) % W + W) % W, y = surf + ((m.y * (floor - surf) + Math.sin(t * 0.5 + m.x * 20) * 6) % (floor - surf));
      ctx.globalAlpha = 0.25 + 0.5 * m.z; ctx.fillRect(x, y, 1.6 * m.z, 1.6 * m.z);
    }
    ctx.globalAlpha = 1;
    // Surface.
    ctx.strokeStyle = "rgba(170,230,255,0.55)"; ctx.lineWidth = 1.5;
    ctx.beginPath();
    for (let x = 0; x <= W; x += 8) ctx.lineTo(x, surf + Math.sin(x * 0.03 + t * 1.6) * 1.5 + Math.sin(x * 0.011 - t) * 1.2);
    ctx.stroke();
    for (const r of rings) { ctx.strokeStyle = `rgba(200,240,255,${r.a * 0.8})`; ctx.beginPath(); ctx.ellipse(r.x, surf, r.r, r.r * 0.18, 0, 0, 7); ctx.stroke(); }
    // Food.
    for (const f of flakes) {
      ctx.save(); ctx.translate(f.x, f.y); ctx.rotate(f.rot); ctx.fillStyle = "#d9892f"; ctx.fillRect(-2.5, -1.5, 5, 3); ctx.restore();
    }
    for (const f of fish) drawFish(f);
    // The finger.
    if (mouse.in) {
      ctx.strokeStyle = "rgba(180,240,255,0.35)"; ctx.lineWidth = 1;
      ctx.beginPath(); ctx.arc(mouse.x, mouse.y, 16 + Math.sin(t * 4) * 2, 0, 7); ctx.stroke();
    }
  }

  animateWhileVisible(canvas, (dt, t) => { resize(); update(dt, t); draw(t); });
}

// ============================================================ 2. ray-marcher
{
  const canvas = $("#rm"), ctx = canvas.getContext("2d");
  const W = canvas.width, H = canvas.height;
  const img = ctx.createImageData(W, H);
  const hist = new Float32Array(W * H * 3), cur = new Float32Array(3);
  const ui = { step: $("#rm-step"), jitter: $("#rm-jitter"), taa: $("#rm-taa"), shadow: $("#rm-shadow"), shafts: $("#rm-shafts") };
  let fresh = true, frame = 0;
  for (const el of Object.values(ui)) el.addEventListener("input", () => { fresh = !ui.taa.checked; $("#rm-step-v").textContent = Math.round(ui.step.value * 100) + " cm"; });

  // Scene (metres): the tank of the app, spheres for rocks, a capsule for the branch.
  const WATER_Y = 0.645, HX = 0.7, HZ = 0.3, SAND = 0.03;
  const spheres = [[-0.28, 0.0, -0.02, 0.17], [0.34, -0.01, -0.1, 0.12], [0.06, -0.01, 0.12, 0.06], [0.56, 0.0, 0.14, 0.07], [-0.56, 0.0, 0.15, 0.05]];
  const capA = [0.16, 0.02, -0.17], capB = [0.08, 0.5, -0.22], capR = 0.035;
  const L = [0.08, 2.05, 0.2];
  const cam = [0, 0.34, 1.3], look = [0, 0.27, 0];
  const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const norm = (a) => { const l = Math.hypot(a[0], a[1], a[2]); return [a[0] / l, a[1] / l, a[2] / l]; };
  const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const fwd = norm(sub(look, cam)), right = norm(cross(fwd, [0, 1, 0])), up = cross(right, fwd);
  const tanF = Math.tan((40 * Math.PI) / 180 / 2), aspect = W / H;
  const spotDir = norm(sub([0, 0, -0.04], L));
  const sideA = norm(cross(spotDir, [0, 0, 1])), sideB = cross(spotDir, sideA);

  function shaft(x, z, t) {
    const qx = x + Math.sin(z * 17 + t * 0.35) * 0.028, qz = z + Math.sin(x * 15 - t * 0.29 + 1.9) * 0.028;
    const fine = Math.sin(qx * 41 + Math.sin(qz * 23 + t * 0.31) * 1.4 + t * 0.23) * Math.sin(qz * 36 - Math.sin(qx * 19 - t * 0.27) * 1.3 - t * 0.19);
    const broad = Math.sin(qx * 7.3 + qz * 4.1 + t * 0.06) * Math.sin(qz * 6.1 - qx * 2.3 - t * 0.05 + 0.7);
    return smoothstep(0.52, 0.9, 0.5 + 0.38 * fine + 0.2 * broad);
  }
  // Does the segment p -> lamp hit a rock or the branch?
  function occluded(px, py, pz) {
    const dx = L[0] - px, dy = L[1] - py, dz = L[2] - pz, len = Math.hypot(dx, dy, dz);
    const ux = dx / len, uy = dy / len, uz = dz / len;
    for (const s of spheres) {
      const ox = px - s[0], oy = py - s[1], oz = pz - s[2];
      const b = ox * ux + oy * uy + oz * uz, c = ox * ox + oy * oy + oz * oz - s[3] * s[3];
      if (c < 0) return true;
      if (b < 0 && b * b - c > 0) return true;
    }
    // Segment-segment distance to the branch axis (sampled: cheap and good enough).
    for (let i = 0; i <= 8; i++) {
      const k = i / 8, ax = capA[0] + (capB[0] - capA[0]) * k, ay = capA[1] + (capB[1] - capA[1]) * k, az = capA[2] + (capB[2] - capA[2]) * k;
      const t = (ax - px) * ux + (ay - py) * uy + (az - pz) * uz;
      if (t > 0) { const qx = px + ux * t - ax, qy = py + uy * t - ay, qz = pz + uz * t - az; if (qx * qx + qy * qy + qz * qz < capR * capR) return true; }
    }
    return false;
  }
  // First opaque hit along the ray: [t, nx, ny, nz, material].
  function hit(o, d, tMax) {
    let best = tMax, n = null, mat = 0;
    if (d[1] < 0) { const t = (SAND - o[1]) / d[1]; if (t > 0 && t < best) { best = t; n = [0, 1, 0]; mat = 1; } }
    for (const s of spheres) {
      const oc = [o[0] - s[0], o[1] - s[1], o[2] - s[2]], b = dot(oc, d), c = dot(oc, oc) - s[3] * s[3], h = b * b - c;
      if (h > 0) { const t = -b - Math.sqrt(h); if (t > 0 && t < best) { best = t; n = norm([o[0] + d[0] * t - s[0], o[1] + d[1] * t - s[1], o[2] + d[2] * t - s[2]]); mat = 2; } }
    }
    // Capsule (iq's analytic intersection).
    const ba = sub(capB, capA), oa = sub(o, capA), baba = dot(ba, ba), bard = dot(ba, d), baoa = dot(ba, oa), rdoa = dot(d, oa), oaoa = dot(oa, oa);
    const a = baba - bard * bard, b = baba * rdoa - baoa * bard, c = baba * oaoa - baoa * baoa - capR * capR * baba, h = b * b - a * c;
    if (h >= 0) {
      const t = (-b - Math.sqrt(h)) / a, y = baoa + t * bard;
      if (y > 0 && y < baba && t > 0 && t < best) { best = t; const p = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t]; const k = y / baba; n = norm(sub(p, [capA[0] + ba[0] * k, capA[1] + ba[1] * k, capA[2] + ba[2] * k])); mat = 3; }
    }
    return [best, n, mat];
  }

  function render(t) {
    const stepLen = +ui.step.value, jitterOn = ui.jitter.checked, taa = ui.taa.checked, shadowsOn = ui.shadow.checked, shaftsOn = ui.shafts.checked;
    const ext = 0.38 + 0.5, dens0 = 0.72;
    let samples = 0;
    frame++;
    for (let py = 0; py < H; py++) {
      for (let px = 0; px < W; px++) {
        // Sub-pixel jitter too, like TAA's camera jitter.
        const jx = jitterOn ? Math.random() - 0.5 : 0, jy = jitterOn ? Math.random() - 0.5 : 0;
        const sx = ((px + 0.5 + jx) / W * 2 - 1) * tanF * aspect, sy = (1 - (py + 0.5 + jy) / H * 2) * tanF;
        const d = norm([fwd[0] + right[0] * sx + up[0] * sy, fwd[1] + right[1] * sx + up[1] * sy, fwd[2] + right[2] * sx + up[2] * sy]);
        // Water box: slab test (the same fix as in the app's shader).
        let t0 = 0, t1 = 1e9;
        const lo = [-HX, 0, -HZ], hi = [HX, WATER_Y, HZ];
        for (let k = 0; k < 3; k++) {
          const inv = 1 / d[k], ta = (lo[k] - cam[k]) * inv, tb = (hi[k] - cam[k]) * inv;
          t0 = Math.max(t0, Math.min(ta, tb)); t1 = Math.min(t1, Math.max(ta, tb));
        }
        let r = 0.008, g = 0.02, b = 0.035;
        if (t1 > t0) {
          const [th, n, mat] = hit(cam, d, t1);
          const len = th - t0;
          const steps = Math.min(16, Math.max(4, Math.ceil(len / stepLen)));
          const h = len / steps;
          const ign = (52.9829189 * ((0.06711056 * (px + 5.588238 * (frame % 64)) + 0.00583715 * (py + 5.588238 * (frame % 64))) % 1)) % 1;
          const off = jitterOn ? ign * 0.9 * h : 0;
          let T = 1, ar = 0, ag = 0, ab = 0;
          for (let i = 0; i < steps; i++) {
            if (T < 0.002) break;
            samples++;
            const tt = t0 + off + i * h, x = cam[0] + d[0] * tt, y = cam[1] + d[1] * tt, z = cam[2] + d[2] * tt;
            const dens = dens0 * (1 + 0.25 * Math.sin(x * 9 + t * 0.2) * Math.sin(z * 11 - t * 0.15) * Math.sin(y * 7));
            const st = Math.exp(-h * dens * ext);
            const lx = L[0] - x, ly = L[1] - y, lz = L[2] - z, ll = Math.hypot(lx, ly, lz);
            const cone = clamp(((-lx * spotDir[0] - ly * spotDir[1] - lz * spotDir[2]) / ll - Math.cos(0.62)) / (Math.cos(0.36) - Math.cos(0.62)), 0, 1);
            let light = cone * cone;
            if (shadowsOn && light > 0) {
              const a = jitterOn ? Math.random() * 2 - 1 : 0, c = jitterOn ? Math.random() * 2 - 1 : 0;
              const qx = x + (sideA[0] * a + sideB[0] * c) * 0.055, qy = y + (sideA[1] * a + sideB[1] * c) * 0.055, qz = z + (sideA[2] * a + sideB[2] * c) * 0.055;
              light *= occluded(qx, qy, qz) ? 0.45 : 1;
            }
            if (shaftsOn && light > 0) {
              const k = (WATER_Y - y) / ly, s = shaft(x + lx * k, z + lz * k, t);
              const sharp = Math.exp(-(WATER_Y - y) * 1.6);
              light *= 0.3 + ((0.12 + 0.88 * s) - 0.3) * sharp;
            } else light *= 0.3; // the shafts' average once blurred
            const scat = light * 2.4, amb = 0.22;
            const w = T * (1 - st) / ext;
            ar += (0.36 * 0.9 * scat + 0.25 * amb) * w;
            ag += (0.80 * scat + 0.55 * amb) * w;
            ab += (0.95 * scat + 1.0 * amb) * w;
            T *= st;
          }
          // What the ray finally hits.
          let sr = 0.012, sg = 0.04, sb = 0.07;
          if (n) {
            const p = [cam[0] + d[0] * th, cam[1] + d[1] * th, cam[2] + d[2] * th];
            const ldir = norm(sub(L, p)), ndl = Math.max(0, dot(n, ldir));
            let lit = ndl * (occluded(p[0] + n[0] * 0.004, p[1] + n[1] * 0.004, p[2] + n[2] * 0.004) ? 0.12 : 1);
            const k = (WATER_Y - p[1]) / (L[1] - p[1]);
            const caust = Math.pow(shaft((p[0] + (L[0] - p[0]) * k) * 2.3, (p[2] + (L[2] - p[2]) * k) * 2.3, t * 1.2), 2);
            lit *= 0.55 + 1.3 * caust;
            const alb = mat === 1 ? [0.58, 0.5, 0.36] : mat === 2 ? [0.2, 0.19, 0.17] : [0.28, 0.18, 0.1];
            sr = alb[0] * (lit * 1.3 + 0.08); sg = alb[1] * (lit * 1.3 + 0.12); sb = alb[2] * (lit * 1.3 + 0.18);
          } else if (d[1] > 0 && Math.abs((cam[1] + d[1] * t1) - WATER_Y) < 1e-3) {
            sr = 0.05; sg = 0.16; sb = 0.24;
          }
          r = sr * T + ar; g = sg * T + ag; b = sb * T + ab;
        }
        const i3 = (py * W + px) * 3;
        if (taa && !fresh) {
          hist[i3] += (r - hist[i3]) * 0.12; hist[i3 + 1] += (g - hist[i3 + 1]) * 0.12; hist[i3 + 2] += (b - hist[i3 + 2]) * 0.12;
        } else { hist[i3] = r; hist[i3 + 1] = g; hist[i3 + 2] = b; }
        const i4 = (py * W + px) * 4;
        img.data[i4] = 255 * Math.pow(1 - Math.exp(-hist[i3] * 1.6), 1 / 2.2);
        img.data[i4 + 1] = 255 * Math.pow(1 - Math.exp(-hist[i3 + 1] * 1.6), 1 / 2.2);
        img.data[i4 + 2] = 255 * Math.pow(1 - Math.exp(-hist[i3 + 2] * 1.6), 1 / 2.2);
        img.data[i4 + 3] = 255;
      }
    }
    fresh = false;
    ctx.putImageData(img, 0, 0);
    $("#rm-samples").textContent = samples.toLocaleString("en");
  }
  animateWhileVisible(canvas, (dt, t) => render(t));
}

// ============================================================ 3. frame budget
{
  const parts = [
    { label: "Water · per-pixel set-up", ms: 2.5, after: 1.8, c: "#5fe1ff", note: "Where does the ray enter and leave the water? It used to test the box's six faces against each other: 36 dot products and branches per pixel. A classic ray-box slab test gives the same answer, <b>−0.7 ms</b>." },
    { label: "Water · pass", ms: 0.6, c: "#51d3f5", note: "Rasterising the water volume and blending it over the scene." },
    { label: "Water · shadow samples", ms: 2.6, after: 2.3, c: "#42bfe4", note: "At every step: is the lamp hidden by a rock or a plant? One hardware-filtered shadow lookup per step, spread by a random offset that TAA smooths out. Constants hoisted out of the loop: <b>−0.3 ms</b>." },
    { label: "Water · density", ms: 1.0, c: "#36a8cf", note: "A 3D noise texture makes the water faintly uneven, like real suspended particles." },
    { label: "Water · light shafts", ms: 0.8, c: "#2b90b6", note: "The surface ripple pattern, evaluated where the lamp's ray crosses the water surface: the same pattern as the caustics on the floor." },
    { label: "Lamp shadows", ms: 1.1, c: "#29c4b8", note: "Rendering the shadow map and sampling it with soft PCSS shadows on every surface. 1024, 2048 or 4096 texels: no measurable difference." },
    { label: "Plants", ms: 2.0, c: "#39d98a", note: "Four procedural species swaying in the vertex shader, drawn in the depth prepass, the shadow map and the main pass. Each species costs under 0.8 ms." },
    { label: "Reflections & sky", ms: 1.6, c: "#8fb3ff", note: "Image-based lighting from a procedural room: the glass, the fish and every glossy surface reflect it." },
    { label: "TAA", ms: 1.3, c: "#b59cff", note: "Temporal anti-aliasing: what turns 4–6 jittered water steps into a smooth volume. Try switching it off in the ray-marcher above." },
    { label: "Bloom", ms: 0.9, c: "#e39cff", note: "The glow around bright caustics and the lamp." },
    { label: "Sharpening", ms: 0.2, c: "#ff9cd0", note: "Contrast-adaptive sharpening." },
    { label: "Everything else", ms: 8.7, c: "#3d5a74", note: "Sand with parallax, 285k triangles of scanned rock, glass, surface, fish, crab, particles, cabinet, the caustics pass and tone mapping. Each is 0.5 ms or less." },
  ];
  const bar = $("#budget-bar"), detail = $("#budget-detail"), total = $("#budget-total");
  const MAX = 25;
  let mode = "after";
  const segs = parts.map((p) => {
    const s = document.createElement("div");
    s.className = "seg"; s.style.background = p.c;
    s.innerHTML = `<span></span>`;
    const show = () => {
      bar.querySelectorAll(".seg").forEach((x) => x.classList.remove("sel")); s.classList.add("sel");
      const v = mode === "after" && p.after ? p.after : p.ms;
      detail.innerHTML = `<b>${v.toFixed(1)} ms</b> · ${p.label}. ${p.note}`;
    };
    s.addEventListener("mouseenter", show); s.addEventListener("click", show);
    bar.appendChild(s);
    return s;
  });
  $("#budget-60").style.left = `${(16.7 / MAX) * 100}%`;
  const marker = document.createElement("div");
  marker.style.cssText = `position:absolute;top:-6px;bottom:-6px;left:${(16.7 / MAX) * 100}%;width:2px;background:#ff7a59;box-shadow:0 0 12px #ff7a59;pointer-events:none`;
  bar.appendChild(marker);
  const pad = document.createElement("div");
  pad.style.cssText = "flex:1 1 auto";
  bar.insertBefore(pad, marker);
  function layout() {
    let sum = 0;
    parts.forEach((p, i) => {
      const v = mode === "after" && p.after ? p.after : p.ms;
      sum += v;
      segs[i].style.flex = `0 0 ${(v / MAX) * 100}%`;
      segs[i].firstChild.textContent = v >= 1.5 ? v.toFixed(1) : "";
    });
    total.textContent = `${sum.toFixed(1)} ms`;
    total.style.left = `${(sum / MAX) * 100}%`;
  }
  document.querySelectorAll(".budget-toggle button").forEach((b) => b.addEventListener("click", () => {
    mode = b.dataset.mode;
    document.querySelectorAll(".budget-toggle button").forEach((x) => x.classList.toggle("on", x === b));
    layout();
  }));
  layout();
}

// ============================================================ 4. DVFS chart
{
  const el = $("#dvfs");
  const data = [
    { res: "50 %", load: 79, cost: 52 },
    { res: "58 %", load: 78, cost: 60 },
    { res: "67 %", load: 78, cost: 71 },
    { res: "100 %", load: 99, cost: 120 },
  ];
  const W = 520, H = 300, pl = 44, pr = 16, pt = 18, pb = 40, max = 130;
  const x = (i) => pl + ((W - pl - pr) / data.length) * (i + 0.5);
  const y = (v) => pt + (1 - v / max) * (H - pt - pb);
  const bw = ((W - pl - pr) / data.length) * 0.46;
  let svg = `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="GPU load reported by macOS versus real cost">`;
  for (const v of [0, 50, 100]) svg += `<line x1="${pl}" x2="${W - pr}" y1="${y(v)}" y2="${y(v)}" stroke="rgba(157,180,201,${v === 100 ? 0.5 : 0.15})" ${v === 100 ? 'stroke-dasharray="4 4"' : ""}/><text x="${pl - 8}" y="${y(v) + 4}" fill="#6b8399" font-size="11" font-family="JetBrains Mono" text-anchor="end">${v}%</text>`;
  svg += `<text x="${pl + 6}" y="${y(100) - 6}" fill="#ff7a59" font-size="11" font-family="JetBrains Mono">one 60 Hz frame</text>`;
  data.forEach((d, i) => {
    svg += `<rect class="b" x="${x(i) - bw / 2}" y="${y(0)}" width="${bw}" height="0" rx="4" fill="${i === 3 ? "#ff7a59" : "#29c4b8"}" data-y="${y(d.load)}" data-h="${y(0) - y(d.load)}"/>`;
    svg += `<text x="${x(i)}" y="${y(d.load) - 8}" fill="#e6f1fb" font-size="13" font-family="JetBrains Mono" text-anchor="middle" class="lbl" opacity="0">${d.load}%</text>`;
    svg += `<text x="${x(i)}" y="${H - 14}" fill="#9db4c9" font-size="12" font-family="Inter" text-anchor="middle">${d.res} res.</text>`;
  });
  svg += `<polyline class="ln" fill="none" stroke="#5fe1ff" stroke-width="2.5" points="${data.map((d, i) => `${x(i)},${y(d.cost)}`).join(" ")}" stroke-dasharray="600" stroke-dashoffset="600"/>`;
  data.forEach((d, i) => { svg += `<circle cx="${x(i)}" cy="${y(d.cost)}" r="4" fill="#5fe1ff"/>`; });
  svg += `<g font-family="Inter" font-size="12"><rect x="${pl + 6}" y="${pt}" width="10" height="10" fill="#29c4b8" rx="2"/><text x="${pl + 22}" y="${pt + 9}" fill="#9db4c9">GPU load macOS reports</text><line x1="${pl + 190}" x2="${pl + 206}" y1="${pt + 5}" y2="${pt + 5}" stroke="#5fe1ff" stroke-width="2.5"/><text x="${pl + 212}" y="${pt + 9}" fill="#9db4c9">real cost at full clock</text></g>`;
  svg += "</svg>";
  el.innerHTML = svg;
  new IntersectionObserver((es, io) => {
    if (!es[0].isIntersecting) return;
    io.disconnect();
    el.querySelectorAll("rect.b").forEach((r, i) => setTimeout(() => {
      r.style.transition = "all 0.9s cubic-bezier(.2,.7,.2,1)";
      r.setAttribute("y", r.dataset.y); r.setAttribute("height", r.dataset.h);
    }, i * 120));
    el.querySelectorAll(".lbl").forEach((t, i) => setTimeout(() => { t.style.transition = "opacity .6s"; t.setAttribute("opacity", 1); }, 500 + i * 120));
    const ln = el.querySelector(".ln");
    setTimeout(() => { ln.style.transition = "stroke-dashoffset 1.6s ease"; ln.style.strokeDashoffset = 0; }, 700);
  }, { threshold: 0.3 }).observe(el);
}

// ============================================================ 5. watts
{
  const el = $("#watts");
  new IntersectionObserver((es, io) => {
    if (!es[0].isIntersecting) return;
    io.disconnect();
    el.querySelectorAll(".w").forEach((w, i) => setTimeout(() => {
      w.querySelector("i").style.width = `${Math.max(1.5, (+w.dataset.w / 12.5) * 100)}%`;
    }, i * 150));
  }, { threshold: 0.3 }).observe(el);
}

// ============================================================ 6. distance field
{
  const canvas = $("#sdf"), ctx = canvas.getContext("2d"), read = $("#sdf-read"), wrap = canvas.parentElement;
  const fields = {};
  let view = "top", field = null, base = null;
  const swimmers = [];

  async function load(name) {
    if (!fields[name]) fields[name] = await (await fetch(`media/sdf_${name}.json`)).json();
    return fields[name];
  }
  // Bilinear distance (mm) at grid coordinates.
  function at(f, gx, gy) {
    const x = clamp(gx, 0, f.w - 1.001), y = clamp(gy, 0, f.h - 1.001), i = Math.floor(x), j = Math.floor(y), u = x - i, v = y - j;
    const d = (a, b) => f.d[b * f.w + a];
    return (d(i, j) * (1 - u) + d(i + 1, j) * u) * (1 - v) + (d(i, j + 1) * (1 - u) + d(i + 1, j + 1) * u) * v;
  }
  function paint() {
    const S = 4, w = field.w * S, h = field.h * S;
    base = document.createElement("canvas"); base.width = w; base.height = h;
    const bctx = base.getContext("2d"), im = bctx.createImageData(w, h);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const d = at(field, x / S, y / S), i = (y * w + x) * 4;
      let r, g, b;
      if (d < 0) { r = 22; g = 36; b = 52; if (((x + y) >> 2) % 3 === 0) { r += 8; g += 10; b += 12; } }
      else {
        const k = Math.exp(-d / 45);
        r = 6 + 60 * k * k; g = 30 + 150 * k; b = 60 + 150 * k;
        const iso = Math.abs(((d % 20) + 20) % 20 - 10);
        if (iso > 9.2) { r += 30; g += 40; b += 40; }
        if (d < 2.5) { r = 180; g = 245; b = 255; }
      }
      im.data[i] = r; im.data[i + 1] = g; im.data[i + 2] = b; im.data[i + 3] = 255;
    }
    bctx.putImageData(im, 0, 0);
  }
  async function setView(v) {
    view = v;
    field = await load(v);
    wrap.classList.toggle("front", v === "front");
    $("#sdf-view").textContent = v === "top" ? "front view" : "top view";
    swimmers.length = 0;
    paint();
  }
  $("#sdf-view").addEventListener("click", () => setView(view === "top" ? "front" : "top"));

  const toGrid = (e) => { const r = canvas.getBoundingClientRect(); return [((e.clientX - r.left) / r.width) * field.w, ((e.clientY - r.top) / r.height) * field.h]; };
  canvas.addEventListener("pointermove", (e) => {
    if (!field) return;
    const [gx, gy] = toGrid(e), d = at(field, gx, gy);
    read.textContent = d < 0 ? `inside the decor · ${(-d).toFixed(0)} mm deep` : `${d.toFixed(0)} mm to the nearest rock, glass or sand`;
  });
  canvas.addEventListener("pointerdown", (e) => {
    if (!field) return;
    const [gx, gy] = toGrid(e);
    if (at(field, gx, gy) < 8) { read.textContent = "that's rock: click in the water"; return; }
    if (swimmers.length > 7) swimmers.shift();
    swimmers.push({ x: gx, y: gy, a: Math.random() * 6.28, w: 0, trail: [], seed: Math.random() * 50 });
  });

  function step(dt, t) {
    if (!field || !base) return;
    const { w, h, dpr } = fit(canvas);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(base, 0, 0, w, h);
    const sx = w / field.w, sy = h / field.h;
    for (const s of swimmers) {
      // Wander, and turn away from obstacles along the field's gradient.
      const e = 0.6, gx = (at(field, s.x + e, s.y) - at(field, s.x - e, s.y)) / (2 * e), gy = (at(field, s.x, s.y + e) - at(field, s.x, s.y - e)) / (2 * e);
      const d = at(field, s.x, s.y);
      let want = s.a + Math.sin(t * 0.7 + s.seed) * 0.6;
      if (d < 70) {
        const away = Math.atan2(gy, gx), k = 1 - d / 70;
        const side = Math.atan2(Math.sin(away - s.a), Math.cos(away - s.a));
        want = s.a + side * k * 1.6;
      }
      const wt = clamp(Math.atan2(Math.sin(want - s.a), Math.cos(want - s.a)) * 3, -3, 3);
      s.w += clamp(wt - s.w, -12 * dt, 12 * dt);
      s.a += s.w * dt;
      const speed = 7; // cells/s = 7 cm/s
      s.x += Math.cos(s.a) * speed * dt; s.y += Math.sin(s.a) * speed * dt;
      if (at(field, s.x, s.y) < 0) { s.a += Math.PI; s.x += Math.cos(s.a) * 0.5; s.y += Math.sin(s.a) * 0.5; }
      s.trail.push([s.x, s.y]); if (s.trail.length > 90) s.trail.shift();
      ctx.strokeStyle = "rgba(255,160,110,0.45)"; ctx.lineWidth = 1.5; ctx.beginPath();
      s.trail.forEach(([x, y], i) => (i ? ctx.lineTo(x * sx, y * sy) : ctx.moveTo(x * sx, y * sy)));
      ctx.stroke();
      ctx.save(); ctx.translate(s.x * sx, s.y * sy); ctx.rotate(s.a);
      ctx.fillStyle = "#ff7a59"; ctx.beginPath(); ctx.ellipse(0, 0, 7, 3, 0, 0, 7); ctx.fill();
      ctx.beginPath(); ctx.moveTo(-6, 0); ctx.lineTo(-11, -3.5); ctx.lineTo(-11, 3.5); ctx.fill();
      ctx.restore();
    }
  }
  setView("top").then(() => animateWhileVisible(canvas, step));
}
