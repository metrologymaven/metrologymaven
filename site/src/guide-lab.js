/* Metrology Maven Executable Field Guide — zero-dependency same-origin WASI host & SVG labs.
 * Runs the real pure-Rust `fid-guide-wasm.wasm` (wasm32-wasip1) directly in the browser.
 * Zero third-party origins (REQ-OUTR.SITE-007); also exportable to Node.js for CI verification.
 */
(function () {
  "use strict";

  const encoder = new TextEncoder();
  const decoder = new TextDecoder("utf-8");

  /**
   * Executes one JSON request object against a compiled WebAssembly.Module of fid-guide-wasm
   * using a minimal, memory-bounded wasi_snapshot_preview1 host (REQ-OUTR.DOCS.GUIDE.SITE-003).
   */
  function runKernelWasm(wasmModule, requestObj) {
    const stdinBytes = encoder.encode(JSON.stringify(requestObj));
    let stdinOffset = 0;
    const stdoutChunks = [];
    let exitCode = 0;
    let memRef = null;

    class ProcExit extends Error {
      constructor(code) {
        super("proc_exit:" + code);
        this.code = code;
      }
    }

    const view = () => new DataView(memRef.buffer);
    const u8 = () => new Uint8Array(memRef.buffer);

    const wasi = {
      args_sizes_get(argcPtr, argvBufSizePtr) {
        const dv = view();
        dv.setUint32(argcPtr, 0, true);
        dv.setUint32(argvBufSizePtr, 0, true);
        return 0;
      },
      args_get() {
        return 0;
      },
      environ_sizes_get(countPtr, bufSizePtr) {
        const dv = view();
        dv.setUint32(countPtr, 0, true);
        dv.setUint32(bufSizePtr, 0, true);
        return 0;
      },
      environ_get() {
        return 0;
      },
      clock_time_get(_id, _prec, timePtr) {
        const dv = view();
        dv.setBigUint64(timePtr, 0n, true);
        return 0;
      },
      random_get(bufPtr, bufLen) {
        // Deterministic zero fill: KNP-1 algorithms never draw from OS randomness.
        u8().fill(0, bufPtr, bufPtr + bufLen);
        return 0;
      },
      fd_read(fd, iovsPtr, iovsLen, nreadPtr) {
        if (fd !== 0) return 8; // EBADF
        const dv = view();
        const bytes = u8();
        let total = 0;
        for (let i = 0; i < iovsLen; i++) {
          const bufPtr = dv.getUint32(iovsPtr + i * 8, true);
          const bufLen = dv.getUint32(iovsPtr + i * 8 + 4, true);
          const avail = stdinBytes.length - stdinOffset;
          if (avail <= 0) break;
          const take = Math.min(bufLen, avail);
          bytes.set(stdinBytes.subarray(stdinOffset, stdinOffset + take), bufPtr);
          stdinOffset += take;
          total += take;
        }
        dv.setUint32(nreadPtr, total, true);
        return 0;
      },
      fd_write(fd, iovsPtr, iovsLen, nwrittenPtr) {
        if (fd !== 1 && fd !== 2) return 8; // EBADF
        const dv = view();
        const bytes = u8();
        let total = 0;
        for (let i = 0; i < iovsLen; i++) {
          const bufPtr = dv.getUint32(iovsPtr + i * 8, true);
          const bufLen = dv.getUint32(iovsPtr + i * 8 + 4, true);
          if (fd === 1 && bufLen > 0) {
            stdoutChunks.push(bytes.slice(bufPtr, bufPtr + bufLen));
          }
          total += bufLen;
        }
        dv.setUint32(nwrittenPtr, total, true);
        return 0;
      },
      proc_exit(code) {
        exitCode = code;
        throw new ProcExit(code);
      },
    };

    const instance = new WebAssembly.Instance(wasmModule, {
      wasi_snapshot_preview1: wasi,
    });
    memRef = instance.exports.memory;
    try {
      instance.exports._start();
    } catch (err) {
      if (!(err instanceof ProcExit)) {
        return {
          exitCode: 3,
          raw: "",
          response: { ok: false, code: "wasm_trap", message: String(err && err.message ? err.message : err) },
        };
      }
    }

    const totalLen = stdoutChunks.reduce((acc, c) => acc + c.length, 0);
    const merged = new Uint8Array(totalLen);
    let pos = 0;
    for (const c of stdoutChunks) {
      merged.set(c, pos);
      pos += c.length;
    }
    const text = decoder.decode(merged);
    if (!text.trim()) {
      return {
        exitCode: exitCode || 3,
        raw: text,
        response: { ok: false, code: "wasm_empty_output", message: "WASM module exited with empty stdout" },
      };
    }
    try {
      return { exitCode, raw: text, response: JSON.parse(text) };
    } catch (err) {
      return {
        exitCode: exitCode || 3,
        raw: text,
        response: { ok: false, code: "wasm_invalid_json", message: String(err && err.message ? err.message : err) },
      };
    }
  }

  // Export for Node.js CI verification (OUTR.DOCS.GUIDE.SITE AC-4).
  if (typeof module !== "undefined" && module.exports) {
    module.exports = { runKernelWasm };
    return;
  }

  // ---------------------------------------------------------------- Browser UI
  const SVG_NS = "http://www.w3.org/2000/svg";
  function el(tag, attrs, children) {
    const node = document.createElement(tag);
    if (attrs) {
      for (const [k, v] of Object.entries(attrs)) {
        if (k === "className") node.className = v;
        else if (k === "textContent") node.textContent = v;
        else if (k.startsWith("on") && typeof v === "function") node.addEventListener(k.slice(2), v);
        else node.setAttribute(k, String(v));
      }
    }
    if (children) {
      for (const ch of children) {
        if (ch == null) continue;
        node.appendChild(typeof ch === "string" ? document.createTextNode(ch) : ch);
      }
    }
    return node;
  }

  function sel(tag, attrs, text) {
    const node = document.createElementNS(SVG_NS, tag);
    if (attrs) {
      for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
    }
    if (text != null) node.textContent = text;
    return node;
  }

  function fmt(n, digits) {
    if (typeof n !== "number" || !Number.isFinite(n)) return "—";
    return n.toFixed(digits == null ? 4 : digits);
  }

  function renderKV(container, rows, digest, modeNote) {
    container.textContent = "";
    const dl = el("div", { className: "lab-kv" });
    for (const [k, v, cls] of rows) {
      const card = el("div", { className: "kv-item" + (cls ? " " + cls : "") }, [
        el("span", { className: "kv-k", textContent: k }),
        el("strong", { className: "kv-v mono", textContent: v }),
      ]);
      dl.appendChild(card);
    }
    container.appendChild(dl);
    if (digest) {
      container.appendChild(
        el("p", { className: "lab-digest mono" }, [
          el("span", { className: "muted", textContent: (modeNote || "kernel") + " digest: " }),
          digest,
        ])
      );
    }
  }

  /**
   * Creates or returns a persistent <svg> inside `wrap` with an inner `<g data-layer>` so
   * pointer capture on the outer `<svg>` stays intact across rapid redraws while dragging.
   */
  function ensurePersistentSvg(wrap, key, W, H, ariaLabel, onPointer) {
    let svg = wrap.querySelector(`svg[data-svg-key="${key}"]`);
    if (!svg) {
      wrap.textContent = "";
      svg = sel("svg", {
        viewBox: `0 0 ${W} ${H}`,
        class: "lab-svg",
        role: "img",
        "aria-label": ariaLabel,
        "data-svg-key": key,
      });
      const layer = sel("g", { "data-layer": "1" });
      svg.appendChild(layer);
      wrap.appendChild(svg);
      let dragIdx = -1;
      const svgCoords = (ev) => {
        const rect = svg.getBoundingClientRect();
        return [
          ((ev.clientX - rect.left) / Math.max(rect.width, 1)) * W,
          ((ev.clientY - rect.top) / Math.max(rect.height, 1)) * H,
        ];
      };
      svg.addEventListener("pointerdown", (ev) => {
        if (!svg._onPointer) return;
        const target = ev.target;
        if (!target || !target.hasAttribute || !target.hasAttribute("data-drag-idx")) return;
        dragIdx = Number(target.getAttribute("data-drag-idx"));
        if (!Number.isInteger(dragIdx) || dragIdx < 0) return;
        ev.preventDefault();
        if (svg.setPointerCapture) svg.setPointerCapture(ev.pointerId);
      });
      svg.addEventListener("pointermove", (ev) => {
        if (dragIdx < 0 || !svg._onPointer) return;
        ev.preventDefault();
        const [sx, sy] = svgCoords(ev);
        svg._onPointer(dragIdx, sx, sy);
      });
      const endDrag = () => {
        dragIdx = -1;
      };
      svg.addEventListener("pointerup", endDrag);
      svg.addEventListener("pointercancel", endDrag);
      svg.addEventListener("lostpointercapture", endDrag);
    }
    svg._onPointer = onPointer || null;
    svg.setAttribute("class", onPointer ? "lab-svg has-drag" : "lab-svg");
    const layer = svg.querySelector("g[data-layer]");
    layer.textContent = "";
    return { svg, layer };
  }

  function initFieldMapFilter() {
    const filterRoot = document.querySelector("[data-field-map-filter]");
    if (!filterRoot) return;
    const cards = Array.from(document.querySelectorAll("[data-pillar-tier]"));
    const buttons = Array.from(filterRoot.querySelectorAll("button[data-tier]"));
    buttons.forEach((btn) => {
      btn.addEventListener("click", () => {
        const want = btn.getAttribute("data-tier");
        buttons.forEach((b) => b.setAttribute("aria-pressed", String(b === btn)));
        cards.forEach((card) => {
          const show = want === "all" || card.getAttribute("data-pillar-tier") === want;
          card.hidden = !show;
        });
      });
    });
  }

  async function initLab() {
    initFieldMapFilter();
    const host = document.getElementById("guide-lab-root");
    const dataEl = document.getElementById("guide-topic-data");
    if (!host || !dataEl) return;

    const topic = JSON.parse(dataEl.textContent);
    const kind = host.getAttribute("data-widget-kind");
    if (!kind || kind === "field-map-explorer") return;

    const wasmUrl = host.getAttribute("data-wasm-url");
    const statusEl = document.getElementById("guide-wasm-status");
    let wasmModule = null;

    if (wasmUrl && typeof WebAssembly !== "undefined") {
      try {
        if (statusEl) statusEl.textContent = "Loading pure-Rust WASM kernel…";
        const resp = await fetch(wasmUrl);
        if (!resp.ok) throw new Error("HTTP " + resp.status);
        const bytes = await resp.arrayBuffer();
        wasmModule = await WebAssembly.compile(bytes);
        if (statusEl) {
          statusEl.textContent = "Live WASM kernel active (fid-guide-wasm · wasm32-wasip1 · D0 bit-exact)";
          statusEl.className = "pill ok";
        }
      } catch (err) {
        if (statusEl) {
          statusEl.textContent = "Static kernel-verified presets (WASM module unavailable: " + err.message + ")";
          statusEl.className = "pill spec";
        }
      }
    } else if (statusEl) {
      statusEl.textContent = "Static kernel-verified presets (build fid-guide-wasm for live sandbox)";
      statusEl.className = "pill spec";
    }

    function evaluate(req, fallbackPreset) {
      if (wasmModule) {
        const out = runKernelWasm(wasmModule, req);
        if (!out.response.ok) {
          return { ok: false, error: out.response.code + ": " + out.response.message, live: true };
        }
        return { ok: true, result: out.response.result, digest: out.response.result_digest, live: true };
      }
      if (fallbackPreset) {
        return { ok: true, result: fallbackPreset.expected_result, digest: fallbackPreset.expected_digest, live: false };
      }
      return { ok: false, error: "Live WASM kernel required for custom edits", live: false };
    }

    const presets = (topic.lab && topic.lab.presets) || [];
    if (kind === "fit-lab") mountFitLab(host, presets, evaluate, !!wasmModule);
    else if (kind === "gdt-zone-lab") mountGdtLab(host, presets, evaluate, !!wasmModule);
    else if (kind === "unc-lab") mountUncLab(host, presets, evaluate, !!wasmModule);
    else if (kind === "decision-lab") mountDecisionLab(host, presets, evaluate, !!wasmModule);
    else if (kind === "canon-lab") mountCanonLab(host, presets, evaluate, !!wasmModule);
  }

  // ---------------------------------------------------------------- Fit Lab
  function mountFitLab(host, presets, evaluate, isLive) {
    host.textContent = "";
    let activeIdx = 0;
    let pts2d = JSON.parse(JSON.stringify(presets[0].request.params.points));
    let excludeOutlier = presets[0].request.params.exclude_index != null;
    let planeVariant = "external_minimax";

    const bar = el("div", { className: "lab-bar" });
    const controls = el("div", { className: "lab-controls" });
    const svgWrap = el("div", { className: "lab-canvas" });
    const readouts = el("div", { className: "lab-readouts" });
    const note = el("p", { className: "muted" });
    host.append(bar, controls, el("div", { className: "lab-grid" }, [svgWrap, readouts]), note);

    function renderPresetButtons() {
      bar.textContent = "";
      presets.forEach((p, idx) => {
        bar.appendChild(
          el("button", {
            type: "button",
            className: "lab-btn" + (idx === activeIdx ? " active" : ""),
            textContent: p.label,
            onclick() {
              activeIdx = idx;
              if (p.op === "fit.circle2d") {
                pts2d = JSON.parse(JSON.stringify(p.request.params.points));
                excludeOutlier = p.request.params.exclude_index != null;
              } else if (p.op === "fit.plane_datum") {
                planeVariant = p.request.params.variant || "external_minimax";
              }
              renderPresetButtons();
              buildControls();
              update();
            },
          })
        );
      });
    }

    function buildControls() {
      const preset = presets[activeIdx];
      controls.textContent = "";
      if (preset.op === "fit.circle2d") {
        const chk = el("input", { type: "checkbox" });
        chk.checked = excludeOutlier;
        chk.disabled = !isLive;
        chk.addEventListener("change", () => {
          excludeOutlier = chk.checked;
          update();
        });
        controls.append(
          el("label", { className: "lab-check" }, [chk, " Compute impact disclosure excluding last point"]),
          el("button", {
            type: "button",
            className: "lab-btn",
            textContent: "Reset points",
            onclick() {
              pts2d = JSON.parse(JSON.stringify(preset.request.params.points));
              update();
            },
          })
        );
      } else if (preset.op === "fit.plane_datum") {
        const selEl = el("select", { className: "lab-select" }, [
          el("option", { value: "external_minimax", textContent: "External Minimax (ISO 5459)" }),
          el("option", { value: "constrained_l1", textContent: "Constrained L1" }),
        ]);
        selEl.value = planeVariant;
        selEl.disabled = !isLive;
        selEl.addEventListener("change", () => {
          planeVariant = selEl.value;
          update();
        });
        controls.append(el("label", {}, ["Tangential variant: ", selEl]));
      }
    }

    function update() {
      const preset = presets[activeIdx];
      note.textContent = preset.description;

      if (preset.op === "fit.circle2d") {
        const req = { op: "fit.circle2d", params: { unit: "mm", points: pts2d } };
        if (excludeOutlier && pts2d.length > 3) req.params.exclude_index = pts2d.length - 1;
        const out = evaluate(req, preset);
        if (!out.ok) {
          readouts.textContent = out.error;
          return;
        }
        const r = out.result;
        drawCircleSvg(svgWrap, pts2d, r, isLive, (idx, nx, ny) => {
          pts2d[idx] = [Number(nx.toFixed(4)), Number(ny.toFixed(4))];
          update();
        });
        const rows = [
          ["Algorithm", r.algorithm_id],
          ["LSQ Diameter", fmt(r.diameter, 5) + " mm"],
          ["Centre (X, Y)", "(" + fmt(r.center_x, 4) + ", " + fmt(r.center_y, 4) + ") mm"],
          ["Residual RMS", fmt(r.rms, 5) + " mm"],
          ["Roundness PV (max-min)", fmt(r.roundness_pv, 5) + " mm"],
        ];
        if (r.without_outlier) {
          rows.push(["Clean Diameter (w/o outlier)", fmt(r.without_outlier.diameter, 5) + " mm", "ok"]);
          rows.push(["Outlier Diameter Shift", fmt(r.without_outlier.diameter_delta, 5) + " mm", "warn"]);
          rows.push(["Centre Shift", fmt(r.without_outlier.center_shift, 5) + " mm", "warn"]);
        }
        renderKV(readouts, rows, out.digest, out.live ? "WASM fid-fit" : "Verified preset");
      } else if (preset.op === "fit.plane_datum") {
        const req = {
          op: "fit.plane_datum",
          params: { unit: "mm", variant: planeVariant, points: preset.request.params.points },
        };
        const out = evaluate(req, preset);
        if (!out.ok) {
          readouts.textContent = out.error;
          return;
        }
        const r = out.result;
        drawPlaneSvg(svgWrap, preset.request.params.points, r);
        renderKV(
          readouts,
          [
            ["LSQ Flatness Span", fmt(r.least_squares.flatness_span, 5) + " mm"],
            ["LSQ RMS", fmt(r.least_squares.rms, 5) + " mm"],
            ["Tangential Objective", fmt(r.tangential.objective, 5) + " mm", "ok"],
            ["Contact Vertex Indices", "[" + r.tangential.contacts.join(", ") + "]"],
            ["LSQ vs. Tangential Tilt", fmt(r.tilt_mrad, 4) + " mrad", "warn"],
            ["Rocking Detected", r.tangential.rocking ? "Yes (" + r.tangential.rock_candidates + ")" : "No (unique)"],
            ["Interpretation (D-045)", r.interpretation_status, "warn"],
          ],
          out.digest,
          out.live ? "WASM fid-fit" : "Verified preset"
        );
      } else if (preset.op === "fit.cylinder_mcc") {
        const out = evaluate(preset.request, preset);
        if (!out.ok) {
          readouts.textContent = out.error;
          return;
        }
        const r = out.result;
        drawCylSvg(svgWrap, preset.request.params.points, r);
        renderKV(
          readouts,
          [
            ["Gaussian LSQ Diameter", fmt(r.least_squares.diameter, 5) + " mm"],
            ["MCC Mating Envelope Ø", fmt(r.minimum_circumscribed.diameter, 5) + " mm", "ok"],
            ["Envelope Excess (MCC - LSQ)", "+" + fmt(r.diameter_envelope_excess, 5) + " mm", "warn"],
            ["MCC Active Contacts", "[" + r.minimum_circumscribed.contacts.join(", ") + "]"],
            ["LSQ Residual RMS", fmt(r.least_squares.rms, 5) + " mm"],
          ],
          out.digest,
          out.live ? "WASM fid-fit" : "Verified preset"
        );
      }
    }

    renderPresetButtons();
    buildControls();
    update();
  }

  function drawCircleSvg(wrap, pts, res, draggable, onDrag) {
    const W = 360, H = 360, CX = W / 2, CY = H / 2, S = 12.5;
    const handler = draggable
      ? (idx, sx, sy) => {
          const mx = Math.max(-13, Math.min(13, (sx - CX) / S));
          const my = Math.max(-13, Math.min(13, (CY - sy) / S));
          onDrag(idx, mx, my);
        }
      : null;
    const { layer } = ensurePersistentSvg(wrap, "fit-circle2d", W, H, "2D circle fit canvas", handler);

    layer.appendChild(sel("line", { x1: 12, y1: CY, x2: W - 12, y2: CY, class: "svg-axis" }));
    layer.appendChild(sel("line", { x1: CX, y1: 12, x2: CX, y2: H - 12, class: "svg-axis" }));

    const toPx = (x, y) => [CX + x * S, CY - y * S];

    if (res.without_outlier) {
      const [cx2, cy2] = toPx(res.without_outlier.center_x, res.without_outlier.center_y);
      layer.appendChild(sel("circle", { cx: cx2, cy: cy2, r: res.without_outlier.radius * S, class: "svg-clean" }));
    }

    const [cx, cy] = toPx(res.center_x, res.center_y);
    layer.appendChild(sel("circle", { cx, cy, r: res.radius * S, class: "svg-fit" }));
    layer.appendChild(sel("line", { x1: cx - 5, y1: cy, x2: cx + 5, y2: cy, class: "svg-center" }));
    layer.appendChild(sel("line", { x1: cx, y1: cy - 5, x2: cx, y2: cy + 5, class: "svg-center" }));

    pts.forEach((p, idx) => {
      const [px, py] = toPx(p[0], p[1]);
      const isEx = res.without_outlier && res.without_outlier.excluded_index === idx;
      const attrs = {
        cx: px,
        cy: py,
        r: isEx ? 7 : 6,
        class: (isEx ? "svg-pt outlier" : "svg-pt") + (draggable ? " draggable" : ""),
      };
      if (draggable) attrs["data-drag-idx"] = String(idx);
      layer.appendChild(sel("circle", attrs));
      layer.appendChild(sel("text", { x: px + 8, y: py - 6, class: "svg-lbl" }, String(idx)));
    });

    layer.appendChild(
      sel("text", { x: 10, y: H - 10, class: "svg-cap" }, draggable ? "Drag points to re-solve live in WASM" : "Static kernel preview")
    );
  }

  function drawPlaneSvg(wrap, pts, res) {
    const W = 360, H = 300;
    const { layer } = ensurePersistentSvg(wrap, "fit-plane", W, H, "Tangential datum plane contacts", null);
    const contacts = new Set(res.tangential.contacts);
    const pad = 42;
    const mapX = (x) => pad + (x / 40) * (W - 2 * pad);
    const mapY = (y) => H - pad - (y / 40) * (H - 2 * pad);

    layer.appendChild(sel("rect", { x: pad, y: pad, width: W - 2 * pad, height: H - 2 * pad, class: "svg-zone" }));
    if (res.tangential.contacts.length >= 3) {
      const cpts = res.tangential.contacts.slice(0, 3).map((idx) => `${mapX(pts[idx][0])},${mapY(pts[idx][1])}`).join(" ");
      layer.appendChild(sel("polygon", { points: cpts, class: "svg-hull" }));
    }
    pts.forEach((p, idx) => {
      const cx = mapX(p[0]), cy = mapY(p[1]);
      const isC = contacts.has(idx);
      layer.appendChild(sel("circle", { cx, cy, r: isC ? 8 : 5.5, class: isC ? "svg-pt outlier" : "svg-pt" }));
      layer.appendChild(sel("text", { x: cx + 9, y: cy - 6, class: "svg-lbl" }, `#${idx} z=${p[2]}`));
    });
    layer.appendChild(sel("text", { x: 10, y: H - 10, class: "svg-cap" }, "Red vertices = KKT-verified tangential datum contacts"));
  }

  function drawCylSvg(wrap, pts, res) {
    const W = 360, H = 320, CX = W / 2, CY = H / 2, R0 = 10.0, BASE = 95, AMP = 1400;
    const { layer } = ensurePersistentSvg(wrap, "fit-cyl", W, H, "Exaggerated radial cylinder comparison", null);
    const rLs = res.least_squares.diameter / 2;
    const rMc = res.minimum_circumscribed.diameter / 2;
    layer.appendChild(sel("circle", { cx: CX, cy: CY, r: BASE + (rMc - R0) * AMP, class: "svg-clean" }));
    layer.appendChild(sel("circle", { cx: CX, cy: CY, r: BASE + (rLs - R0) * AMP, class: "svg-fit" }));
    const contacts = new Set(res.minimum_circumscribed.contacts);
    pts.slice(0, 8).forEach((p, idx) => {
      const ang = Math.atan2(p[1], p[0]);
      const rad = Math.hypot(p[0], p[1]);
      const pr = BASE + (rad - R0) * AMP;
      const isC = contacts.has(idx) || contacts.has(idx + 8) || contacts.has(idx + 16);
      layer.appendChild(sel("circle", { cx: CX + pr * Math.cos(ang), cy: CY - pr * Math.sin(ang), r: isC ? 6.5 : 4.5, class: isC ? "svg-pt outlier" : "svg-pt" }));
    });
    layer.appendChild(sel("text", { x: 10, y: H - 10, class: "svg-cap" }, "Radial form exaggerated 1400x: green = MCC mating envelope, blue = LSQ"));
  }

  // ---------------------------------------------------------------- GD&T Lab
  function mountGdtLab(host, presets, evaluate, isLive) {
    host.textContent = "";
    let activeIdx = 0;
    let tolVal = presets[0].request.params.tolerance;
    let pts = JSON.parse(JSON.stringify(presets[0].request.params.points));
    let devs = presets[0].request.params.deviations ? presets[0].request.params.deviations.slice() : null;
    let currentScale = 1000;

    const bar = el("div", { className: "lab-bar" });
    const controls = el("div", { className: "lab-controls" });
    const svgWrap = el("div", { className: "lab-canvas" });
    const readouts = el("div", { className: "lab-readouts" });
    const note = el("p", { className: "muted" });
    host.append(bar, controls, el("div", { className: "lab-grid" }, [svgWrap, readouts]), note);

    // Build persistent controls ONCE (never rebuilt on slider input).
    const slider = el("input", { type: "range", min: "0.02", max: "0.30", step: "0.005", value: String(tolVal) });
    slider.disabled = !isLive;
    const valSpan = el("span", { className: "mono", textContent: fmt(tolVal, 3) + " mm" });
    slider.addEventListener("input", () => {
      tolVal = Number(slider.value);
      valSpan.textContent = fmt(tolVal, 3) + " mm";
      update();
    });
    const resetBtn = el("button", {
      type: "button",
      className: "lab-btn",
      textContent: "Reset points",
      onclick() {
        const p = presets[activeIdx];
        tolVal = p.request.params.tolerance;
        pts = JSON.parse(JSON.stringify(p.request.params.points));
        devs = p.request.params.deviations ? p.request.params.deviations.slice() : null;
        slider.value = String(tolVal);
        valSpan.textContent = fmt(tolVal, 3) + " mm";
        update();
      },
    });
    controls.append(
      el("label", { className: "lab-slider" }, ["Tolerance zone callout: ", slider, " ", valSpan]),
      resetBtn
    );

    function renderBar() {
      bar.textContent = "";
      presets.forEach((p, idx) => {
        bar.appendChild(
          el("button", {
            type: "button",
            className: "lab-btn" + (idx === activeIdx ? " active" : ""),
            textContent: p.label,
            onclick() {
              activeIdx = idx;
              tolVal = p.request.params.tolerance;
              pts = JSON.parse(JSON.stringify(p.request.params.points));
              devs = p.request.params.deviations ? p.request.params.deviations.slice() : null;
              slider.value = String(tolVal);
              valSpan.textContent = fmt(tolVal, 3) + " mm";
              renderBar();
              update();
            },
          })
        );
      });
    }

    function update() {
      const preset = presets[activeIdx];
      note.textContent = preset.description;

      const req = JSON.parse(JSON.stringify(preset.request));
      req.params.tolerance = tolVal;
      req.params.points = pts;
      if (devs) req.params.deviations = devs;

      const out = evaluate(req, preset);
      if (!out.ok) {
        readouts.textContent = out.error;
        return;
      }
      const r = out.result;
      currentScale = 95 / Math.max(r.tolerance, 0.12);
      drawZoneSvg(svgWrap, req.params, r, currentScale, isLive, (idx, sx, sy) => {
        const W = 360, H = 300, CX = W / 2, CY = H / 2;
        if (r.shape === "cylinder" || r.shape === "circle" || r.shape === "sphere") {
          const nx = Math.max(-0.2, Math.min(0.2, (sx - CX) / currentScale));
          const ny = Math.max(-0.2, Math.min(0.2, (CY - sy) / currentScale));
          pts[idx][0] = Number(nx.toFixed(5));
          pts[idx][1] = Number(ny.toFixed(5));
        } else {
          const shift = req.params.disposition_shift || 0;
          const nv = Math.max(-0.25, Math.min(0.25, shift + (CY - sy) / currentScale));
          if (r.shape === "offset" && devs) devs[idx] = Number(nv.toFixed(5));
          else pts[idx][2] = Number(nv.toFixed(5));
        }
        update();
      });
      renderKV(
        readouts,
        [
          ["Zone Shape", r.shape],
          ["Dialect Context", r.dialect],
          ["Full Zone Usage", fmt(r.usage_size, 5) + " mm", r.contains ? "ok" : "bad"],
          ["Callout Limit", fmt(r.tolerance, 4) + " mm"],
          ["Utilization", fmt(r.utilization_pct, 1) + "%", r.contains ? "ok" : "bad"],
          ["Driving Point Index", "#" + r.driving_index, "warn"],
          ["Containment Verdict", r.contains ? "INSIDE ZONE" : "EXCEEDS ZONE", r.contains ? "ok" : "bad"],
          ["Interpretation (D-045)", r.interpretation_status, "warn"],
        ],
        out.digest,
        out.live ? "WASM fid-gps" : "Verified preset"
      );
    }

    renderBar();
    update();
  }

  function drawZoneSvg(wrap, params, res, scale, draggable, onDrag) {
    const W = 360, H = 300, CX = W / 2, CY = H / 2;
    const { layer } = ensurePersistentSvg(
      wrap,
      "gdt-zone",
      W,
      H,
      "GD&T tolerance zone visualization",
      draggable ? onDrag : null
    );
    layer.appendChild(sel("line", { x1: 16, y1: CY, x2: W - 16, y2: CY, class: "svg-axis" }));
    layer.appendChild(sel("line", { x1: CX, y1: 16, x2: CX, y2: H - 16, class: "svg-axis" }));

    if (res.shape === "cylinder" || res.shape === "circle" || res.shape === "sphere") {
      layer.appendChild(sel("circle", { cx: CX, cy: CY, r: (res.tolerance / 2) * scale, class: "svg-zone" }));
      layer.appendChild(sel("circle", { cx: CX, cy: CY, r: (res.usage_size / 2) * scale, class: res.contains ? "svg-clean" : "svg-bad" }));
      params.points.forEach((p, idx) => {
        const px = CX + p[0] * scale;
        const py = CY - p[1] * scale;
        const drv = idx === res.driving_index;
        const attrs = {
          cx: px,
          cy: py,
          r: drv ? 7 : 5.5,
          class: (drv ? "svg-pt outlier" : "svg-pt") + (draggable ? " draggable" : ""),
        };
        if (draggable) attrs["data-drag-idx"] = String(idx);
        layer.appendChild(sel("circle", attrs));
        layer.appendChild(sel("text", { x: px + 8, y: py - 5, class: "svg-lbl" }, `#${idx}`));
      });
    } else {
      const vals = res.shape === "offset" ? params.deviations : params.points.map((p) => p[2]);
      const shift = params.disposition_shift || 0;
      const yOf = (v) => CY - (v - shift) * scale;
      const halfH = (res.tolerance / 2) * scale;
      layer.appendChild(sel("rect", { x: 30, y: CY - halfH, width: W - 60, height: 2 * halfH, class: "svg-zone" }));
      vals.forEach((v, idx) => {
        const px = 55 + (idx / Math.max(1, vals.length - 1)) * (W - 110);
        const py = yOf(v);
        const drv = idx === res.driving_index;
        const attrs = {
          cx: px,
          cy: py,
          r: drv ? 7 : 5.5,
          class: (drv ? "svg-pt outlier" : "svg-pt") + (draggable ? " draggable" : ""),
        };
        if (draggable) attrs["data-drag-idx"] = String(idx);
        layer.appendChild(sel("circle", attrs));
        layer.appendChild(sel("text", { x: px + 8, y: py - 5, class: "svg-lbl" }, `#${idx}`));
      });
    }
    const hint = draggable ? "Drag points · Red = driving point (#" + res.driving_index + ")" : "Red = driving point (#" + res.driving_index + ")";
    layer.appendChild(sel("text", { x: 10, y: H - 10, class: "svg-cap" }, hint));
  }

  // ---------------------------------------------------------------- Uncertainty Lab
  function mountUncLab(host, presets, evaluate, isLive) {
    host.textContent = "";
    let activeIdx = 0;
    let trials = presets[0].request.params.trials;
    let s1 = 1.5;

    const bar = el("div", { className: "lab-bar" });
    const controls = el("div", { className: "lab-controls" });
    const svgWrap = el("div", { className: "lab-canvas" });
    const readouts = el("div", { className: "lab-readouts" });
    const note = el("p", { className: "muted" });
    host.append(bar, controls, el("div", { className: "lab-grid" }, [svgWrap, readouts]), note);

    // Persistent controls built once.
    const sl = el("input", { type: "range", min: "0.5", max: "12.0", step: "0.5", value: String(s1) });
    sl.disabled = !isLive;
    const sp = el("span", { className: "mono", textContent: fmt(s1, 1) + " um" });
    sl.addEventListener("input", () => {
      s1 = Number(sl.value);
      sp.textContent = fmt(s1, 1) + " um";
      update();
    });
    const trSel = el("select", { className: "lab-select" }, [
      el("option", { value: "4000", textContent: "4,000 trials" }),
      el("option", { value: "8000", textContent: "8,000 trials" }),
      el("option", { value: "16000", textContent: "16,000 trials" }),
    ]);
    trSel.value = String(trials);
    trSel.disabled = !isLive;
    trSel.addEventListener("change", () => {
      trials = Number(trSel.value);
      update();
    });
    controls.append(
      el("label", { className: "lab-slider" }, ["Primary input scale: ", sl, " ", sp]),
      el("label", {}, ["MC trials: ", trSel])
    );

    function syncControls(p) {
      trials = p.request.params.trials;
      const i0 = p.request.params.inputs[0];
      s1 = Number(((i0.sigma != null ? i0.sigma : i0.half_width) * 1e6).toFixed(6));
      sl.value = String(s1);
      sp.textContent = fmt(s1, 1) + " um";
      trSel.value = String(trials);
    }

    function renderBar() {
      bar.textContent = "";
      presets.forEach((p, idx) => {
        bar.appendChild(
          el("button", {
            type: "button",
            className: "lab-btn" + (idx === activeIdx ? " active" : ""),
            textContent: p.label,
            onclick() {
              activeIdx = idx;
              syncControls(p);
              renderBar();
              update();
            },
          })
        );
      });
    }

    function update() {
      const preset = presets[activeIdx];
      note.textContent = preset.description;

      const req = JSON.parse(JSON.stringify(preset.request));
      req.params.trials = trials;
      const scaled = Number((s1 * 1e-6).toFixed(9));
      if (req.params.inputs[0].sigma != null) req.params.inputs[0].sigma = scaled;
      else req.params.inputs[0].half_width = scaled;

      const out = evaluate(req, preset);
      if (!out.ok) {
        readouts.textContent = out.error;
        return;
      }
      const r = out.result;
      drawHistSvg(svgWrap, r);
      renderKV(
        readouts,
        [
          ["Model", r.model + " (" + r.trials + " trials)"],
          ["LPU Standard u_c", fmt(r.lpu_u_std * 1e6, 3) + " um"],
          ["MCM Standard u", fmt(r.mcm_u_std * 1e6, 3) + " um"],
          ["LPU 95% Interval", "[" + fmt(r.lpu_low * 1e3, 5) + ", " + fmt(r.lpu_high * 1e3, 5) + "] mm"],
          ["MCM 95% Interval", "[" + fmt(r.mcm_sym_low * 1e3, 5) + ", " + fmt(r.mcm_sym_high * 1e3, 5) + "] mm"],
          ["JCGM 101 LPU Check", r.lpu_check_passed ? "PASS (LPU adequate)" : "FAIL (use Monte Carlo)", r.lpu_check_passed ? "ok" : "bad"],
          ["Evidence Class", r.evidence_class],
        ],
        out.digest,
        out.live ? "WASM fid-kunc" : "Verified preset"
      );
    }

    syncControls(presets[0]);
    renderBar();
    update();
  }

  function drawHistSvg(wrap, r) {
    const W = 380, H = 260, padL = 24, padR = 16, padT = 24, padB = 34;
    const { layer } = ensurePersistentSvg(wrap, "unc-hist", W, H, "Monte Carlo distribution histogram", null);
    const bins = r.histogram || [];
    if (!bins.length) return;
    const lo = Math.min(bins[0].low, r.lpu_low);
    const hi = Math.max(bins[bins.length - 1].high, r.lpu_high);
    const span = Math.max(hi - lo, 1e-12);
    const maxC = Math.max(...bins.map((b) => b.count), 1);
    const xOf = (v) => padL + ((v - lo) / span) * (W - padL - padR);
    const chartH = H - padT - padB;

    bins.forEach((b) => {
      const x1 = xOf(b.low);
      const x2 = xOf(b.high);
      const bh = (b.count / maxC) * chartH;
      layer.appendChild(sel("rect", { x: x1, y: padT + chartH - bh, width: Math.max(1, x2 - x1 - 1), height: bh, class: "svg-bar" }));
    });
    [r.lpu_low, r.lpu_high].forEach((v) => {
      layer.appendChild(sel("line", { x1: xOf(v), y1: padT, x2: xOf(v), y2: padT + chartH, class: "svg-lpu" }));
    });
    [r.mcm_sym_low, r.mcm_sym_high].forEach((v) => {
      layer.appendChild(sel("line", { x1: xOf(v), y1: padT, x2: xOf(v), y2: padT + chartH, class: "svg-mcm" }));
    });
    layer.appendChild(sel("text", { x: padL, y: H - 10, class: "svg-cap" }, "Dashed blue = LPU 95% · Solid red = Monte Carlo 95%"));
  }

  // ---------------------------------------------------------------- Decision Lab
  function mountDecisionLab(host, presets, evaluate, isLive) {
    host.textContent = "";
    let activeIdx = 0;
    let valMm = 10.045;
    let uUm = 5.0;
    let ruleRef = "dr:simple-acceptance@1";
    let uec0 = false;

    const bar = el("div", { className: "lab-bar" });
    const controls = el("div", { className: "lab-controls" });
    const svgWrap = el("div", { className: "lab-canvas" });
    const readouts = el("div", { className: "lab-readouts" });
    const note = el("p", { className: "muted" });
    host.append(bar, controls, el("div", { className: "lab-grid" }, [svgWrap, readouts]), note);

    // Build persistent controls ONCE so range sliders never abort mid-slide.
    const slY = el("input", { type: "range", min: "9.930", max: "10.070", step: "0.001", value: String(valMm) });
    slY.disabled = !isLive;
    const spY = el("span", { className: "mono", textContent: fmt(valMm, 3) + " mm" });
    slY.addEventListener("input", () => {
      valMm = Number(slY.value);
      spY.textContent = fmt(valMm, 3) + " mm";
      update();
    });

    const slU = el("input", { type: "range", min: "1", max: "15", step: "0.5", value: String(uUm) });
    slU.disabled = !isLive;
    const spU = el("span", { className: "mono", textContent: fmt(uUm, 1) + " um" });
    slU.addEventListener("input", () => {
      uUm = Number(slU.value);
      spU.textContent = fmt(uUm, 1) + " um";
      update();
    });

    const rSel = el("select", { className: "lab-select" }, [
      el("option", { value: "dr:simple-acceptance@1", textContent: "Simple acceptance (shared risk)" }),
      el("option", { value: "dr:iso14253-1:2017/default@1", textContent: "ISO 14253-1 default (p_min = 0.95)" }),
      el("option", { value: "dr:guard-band(r=1)@1", textContent: "Guard band r = 1.0 (strict)" }),
      el("option", { value: "dr:guard-band(r=0.5)@1", textContent: "Guard band r = 0.5" }),
      el("option", { value: "dr:as-reported-by-source@1", textContent: "Label only (transcription)" }),
    ]);
    rSel.disabled = !isLive;
    rSel.addEventListener("change", () => {
      ruleRef = rSel.value;
      update();
    });

    const chk0 = el("input", { type: "checkbox" });
    chk0.disabled = !isLive;
    chk0.addEventListener("change", () => {
      uec0 = chk0.checked;
      slU.disabled = !isLive || uec0;
      update();
    });

    controls.append(
      el("label", { className: "lab-slider" }, ["Measured y: ", slY, " ", spY]),
      el("label", { className: "lab-slider" }, ["u_std: ", slU, " ", spU]),
      el("label", {}, ["Rule: ", rSel]),
      el("label", { className: "lab-check" }, [chk0, " UEC-0 (missing U)"])
    );

    function syncFromPreset(p) {
      valMm = Number((p.request.params.value * 1e3).toFixed(6));
      uUm = Number(((p.request.params.u_std || 0.000005) * 1e6).toFixed(6));
      ruleRef = p.request.params.rule_ref;
      uec0 = !!p.request.params.uec0;
      slY.value = String(valMm);
      spY.textContent = fmt(valMm, 3) + " mm";
      slU.value = String(uUm);
      spU.textContent = fmt(uUm, 1) + " um";
      slU.disabled = !isLive || uec0;
      rSel.value = ruleRef;
      chk0.checked = uec0;
    }

    function renderBar() {
      bar.textContent = "";
      presets.forEach((p, idx) => {
        bar.appendChild(
          el("button", {
            type: "button",
            className: "lab-btn" + (idx === activeIdx ? " active" : ""),
            textContent: p.label,
            onclick() {
              activeIdx = idx;
              syncFromPreset(p);
              renderBar();
              update();
            },
          })
        );
      });
    }

    function update() {
      const preset = presets[activeIdx];
      note.textContent = preset.description;

      const req = {
        op: "dec.evaluate",
        params: {
          value: Number((valMm * 1e-3).toFixed(9)),
          lsl: "0.009950",
          usl: "0.010050",
          rule_ref: ruleRef,
          uec0,
          prior_mu: 0.01,
          prior_sigma: 0.000018,
        },
      };
      if (!uec0) {
        req.params.u_std = Number((uUm * 1e-6).toFixed(9));
        req.params.k = 2.0;
      }

      const out = evaluate(req, preset);
      if (!out.ok) {
        readouts.textContent = out.error;
        return;
      }
      const r = out.result;
      drawDecisionSvg(svgWrap, r);
      const cls = r.outcome === "CONFORMS" ? "ok" : r.outcome === "NONCONFORMS" ? "bad" : "warn";
      const rows = [
        ["Outcome", r.outcome, cls],
        ["Rule", r.rule_ref],
        ["Evidence Class", r.evidence_class],
        ["Conformance Prob. p", r.p_conformance != null ? fmt(r.p_conformance * 100, 2) + "%" : "—"],
        ["Specific Consumer Risk", r.specific_consumer_risk != null ? fmt(r.specific_consumer_risk * 100, 2) + "%" : "—"],
        ["Specific Producer Risk", r.specific_producer_risk != null ? fmt(r.specific_producer_risk * 100, 2) + "%" : "—"],
        ["Global Consumer Risk", r.global_consumer_risk != null ? fmt(r.global_consumer_risk * 100, 3) + "%" : "—"],
        ["Global Producer Risk", r.global_producer_risk != null ? fmt(r.global_producer_risk * 100, 3) + "%" : "—"],
        ["Flags", r.flags && r.flags.length ? r.flags.join(", ") : "none"],
        ["Interpretation (D-045)", r.interpretation_status, "warn"],
      ];
      renderKV(readouts, rows, out.digest, out.live ? "WASM fid-kdec" : "Verified preset");
    }

    syncFromPreset(presets[0]);
    renderBar();
    update();
  }

  function drawDecisionSvg(wrap, r) {
    const W = 380, H = 250, padL = 22, padR = 22, baseY = 195;
    const { layer } = ensurePersistentSvg(wrap, "dec-curve", W, H, "Tolerance band and uncertainty PDF", null);
    const lo = 9.92, hi = 10.08;
    const xOf = (mm) => padL + ((mm - lo) / (hi - lo)) * (W - padL - padR);
    const lsl = Number(r.lsl) * 1e3, usl = Number(r.usl) * 1e3, y = r.value * 1e3;

    layer.appendChild(sel("rect", { x: xOf(lsl), y: 26, width: xOf(usl) - xOf(lsl), height: baseY - 26, class: "svg-zone" }));
    if (r.acceptance_low != null && r.acceptance_high != null && r.acceptance_high > r.acceptance_low) {
      layer.appendChild(
        sel("rect", {
          x: xOf(r.acceptance_low * 1e3),
          y: 26,
          width: xOf(r.acceptance_high * 1e3) - xOf(r.acceptance_low * 1e3),
          height: baseY - 26,
          class: "svg-accept",
        })
      );
    }
    layer.appendChild(sel("line", { x1: xOf(lsl), y1: 20, x2: xOf(lsl), y2: baseY, class: "svg-mcm" }));
    layer.appendChild(sel("line", { x1: xOf(usl), y1: 20, x2: xOf(usl), y2: baseY, class: "svg-mcm" }));
    layer.appendChild(sel("text", { x: xOf(lsl) - 12, y: baseY + 15, class: "svg-lbl" }, "LSL"));
    layer.appendChild(sel("text", { x: xOf(usl) - 12, y: baseY + 15, class: "svg-lbl" }, "USL"));

    // Measured value marker (never fabricate a Gaussian PDF when UEC-0 / expanded_u is absent).
    layer.appendChild(sel("line", { x1: xOf(y), y1: 35, x2: xOf(y), y2: baseY, class: "svg-center" }));
    if (r.expanded_u != null && r.evidence_class !== "UEC-0") {
      const uMm = (r.expanded_u / 2) * 1e3;
      const pts = [];
      for (let i = 0; i <= 120; i++) {
        const mm = lo + (i / 120) * (hi - lo);
        const z = (mm - y) / Math.max(uMm, 0.0005);
        const py = baseY - Math.exp(-0.5 * z * z) * 135;
        pts.push(`${xOf(mm).toFixed(1)},${py.toFixed(1)}`);
      }
      layer.appendChild(sel("polyline", { points: pts.join(" "), class: "svg-fit" }));
      layer.appendChild(sel("text", { x: 10, y: H - 10, class: "svg-cap" }, "Green = acceptance zone · Red = [LSL, USL] · Curve = N(y, u_std)"));
    } else {
      layer.appendChild(sel("text", { x: 76, y: 112, class: "svg-lbl" }, "UEC-0: uncertainty missing (no PDF fabricated)"));
      layer.appendChild(sel("text", { x: 10, y: H - 10, class: "svg-cap" }, "Red = [LSL, USL] · Vertical line = y (UEC-0 -> NOT_DECIDABLE)"));
    }
  }

  // ---------------------------------------------------------------- Canon Lab
  function mountCanonLab(host, presets, evaluate, isLive) {
    host.textContent = "";
    let activeIdx = 0;
    const bar = el("div", { className: "lab-bar" });
    const taA = el("textarea", { className: "lab-ta mono", rows: "4", "aria-label": "JSON Document A" });
    const taB = el("textarea", { className: "lab-ta mono", rows: "4", "aria-label": "JSON Document B (optional)" });
    taA.readOnly = !isLive;
    taB.readOnly = !isLive;
    const readouts = el("div", { className: "lab-readouts" });
    const note = el("p", { className: "muted" });

    const editors = el("div", { className: "two" }, [
      el("div", { className: "col" }, [el("h3", { textContent: "Document A" }), taA]),
      el("div", { className: "col" }, [el("h3", { textContent: "Document B (leave blank for single-doc hash)" }), taB]),
    ]);
    host.append(bar, editors, readouts, note);

    function loadPreset(idx) {
      activeIdx = idx;
      const p = presets[idx];
      taA.value = p.request.params.json_a;
      taB.value = p.request.params.json_b || "";
      renderBar();
      update();
    }

    function renderBar() {
      bar.textContent = "";
      presets.forEach((p, idx) => {
        bar.appendChild(
          el("button", {
            type: "button",
            className: "lab-btn" + (idx === activeIdx ? " active" : ""),
            textContent: p.label,
            onclick: () => loadPreset(idx),
          })
        );
      });
    }

    function update() {
      const preset = presets[activeIdx];
      note.textContent = preset.description;
      const params = { json_a: taA.value };
      if (taB.value.trim() !== "") params.json_b = taB.value;
      const req = { op: "canon.digest", params };
      const out = evaluate(req, preset);
      if (!out.ok) {
        renderKV(readouts, [["Parse Status", out.error, "bad"]], null);
        return;
      }
      const r = out.result;
      const verdictCls = r.replay_verdict === "IDENTICAL" ? "ok" : r.replay_verdict === "DIVERGENT" ? "bad" : "p";
      const rows = [
        ["Replay Verdict", r.replay_verdict || "SINGLE DOCUMENT", verdictCls],
        ["Canonical A (" + r.byte_length_a + " B)", r.canonical_a],
        ["SHA-256 Digest A", r.digest_a],
      ];
      if (r.canonical_b != null) {
        rows.push(["Canonical B (" + (r.byte_length_b || 0) + " B)", r.canonical_b]);
        rows.push(["SHA-256 Digest B", r.digest_b]);
      }
      renderKV(readouts, rows, out.digest, out.live ? "WASM fid-canon" : "Verified preset");
    }

    taA.addEventListener("input", update);
    taB.addEventListener("input", update);
    loadPreset(0);
  }

  if (typeof document !== "undefined") {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", initLab);
    } else {
      initLab();
    }
  }
})();
