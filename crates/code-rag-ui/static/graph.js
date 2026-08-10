// R5 topology renderer: d3-force + canvas, driven through the same
// global-function bridge pattern as embedder.js/reranker.js.
//
// Contract with src/graph_bridge.rs (singleton — one graph at a time):
//   __codeRagGraphInit(canvas, dataJson, onNodeClick) -> true / throws
//   __codeRagGraphDestroy()
//   __codeRagGraphResize()
//   __codeRagGraphSetVisible(bool)
//
// Colors come from the --viz-* CSS custom properties (single source of truth,
// re-read on data-theme changes via a MutationObserver), so the graph re-skins
// live with the rest of the app.

let state = null;

const LEGEND_SLOTS = 8;
// Edge dash by dominant relation; precedence must match the artifact contract:
// calls > imports/re_exports > type links > contains.
const RELATION_STYLE = [
    { match: (r) => r.includes("calls"), dash: [] },
    { match: (r) => r.includes("imports") || r.includes("re_exports"), dash: [6, 3] },
    {
        match: (r) =>
            r.includes("implements") || r.includes("extends") || r.includes("embeds") ||
            r.includes("references") || r.includes("rationale_for"),
        dash: [2, 3],
    },
    { match: () => true, dash: [1, 3] }, // contains / unknown — faintest
];
const ALPHA_EXTRACTED = 0.55;
const ALPHA_INFERRED = 0.28;

function cssVar(name) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function readPalette() {
    const communities = [];
    for (let i = 1; i <= LEGEND_SLOTS; i++) communities.push(cssVar(`--viz-c${i}`));
    return {
        communities,
        other: cssVar("--viz-other"),
        edge: cssVar("--viz-edge"),
        surface: `rgb(${cssVar("--surface")})`,
        label: `rgb(${cssVar("--gray-dark")})`,
    };
}

function nodeColor(node, palette) {
    if (node.community == null || node.community >= LEGEND_SLOTS) return palette.other;
    return palette.communities[node.community];
}

function dashFor(relations) {
    for (const s of RELATION_STYLE) if (s.match(relations || [])) return s.dash;
    return [1, 3];
}

function nodeRadius(degree, maxDegree) {
    // sqrt scale, 3–14 px — perceptual area encoding.
    const t = Math.sqrt(Math.max(degree, 0) / Math.max(maxDegree, 1));
    return 3 + t * 11;
}

window.__codeRagGraphInit = async function (canvas, dataJson, onNodeClick) {
    const [force, selection, zoomMod] = await Promise.all([
        import("https://cdn.jsdelivr.net/npm/d3-force@3/+esm"),
        import("https://cdn.jsdelivr.net/npm/d3-selection@3/+esm"),
        import("https://cdn.jsdelivr.net/npm/d3-zoom@3/+esm"),
    ]);
    window.__codeRagGraphDestroy();

    const data = JSON.parse(dataJson);
    const nodes = data.nodes.map((n) => ({ ...n }));
    const edges = data.edges.map((e) => ({ ...e }));
    const maxDegree = nodes.reduce((m, n) => Math.max(m, n.degree || 0), 1);

    const ctx = canvas.getContext("2d");
    const tooltip = document.createElement("div");
    tooltip.className = "graph-tooltip";
    document.body.appendChild(tooltip);

    const s = {
        canvas, ctx, nodes, edges, maxDegree, tooltip,
        palette: readPalette(),
        transform: zoomMod.zoomIdentity,
        visible: true,
        needsDraw: true,
        raf: 0,
        hovered: null,
        listeners: [],
        observer: null,
        sim: null,
        zoom: null,
        dpr: window.devicePixelRatio || 1,
        width: 0,
        height: 0,
    };
    state = s;

    resizeCanvas(s);

    s.sim = force
        .forceSimulation(nodes)
        .force(
            "link",
            force
                .forceLink(edges)
                .id((d) => d.id)
                .distance((e) => (dashFor(e.relations).length && dashFor(e.relations)[0] === 1 ? 22 : 42))
                .strength(0.4),
        )
        .force("charge", force.forceManyBody().strength(-28).theta(0.9))
        .force("center", force.forceCenter(s.width / 2, s.height / 2))
        .force("collide", force.forceCollide((d) => nodeRadius(d.degree, maxDegree) + 1.5))
        .alphaDecay(0.035)
        .on("tick", () => { s.needsDraw = true; });

    // Pan/zoom (d3-zoom suppresses click-after-drag for free).
    const sel = selection.select(canvas);
    s.zoom = zoomMod
        .zoom()
        .scaleExtent([0.15, 8])
        .on("zoom", (ev) => {
            s.transform = ev.transform;
            s.needsDraw = true;
        });
    sel.call(s.zoom);

    const on = (target, type, fn) => {
        target.addEventListener(type, fn);
        s.listeners.push([target, type, fn]);
    };

    on(canvas, "mousemove", (ev) => {
        const n = pick(s, ev);
        if (n !== s.hovered) {
            s.hovered = n;
            s.needsDraw = true;
        }
        if (n) {
            tooltip.style.display = "block";
            tooltip.style.left = `${ev.clientX + 14}px`;
            tooltip.style.top = `${ev.clientY + 14}px`;
            const community = n.community == null ? "—" : n.community;
            tooltip.innerHTML =
                `<div class="tt-title"></div><div class="tt-file"></div>` +
                `<div class="tt-meta"></div>`;
            tooltip.querySelector(".tt-title").textContent = n.label || n.id;
            tooltip.querySelector(".tt-file").textContent = n.file || "";
            tooltip.querySelector(".tt-meta").textContent =
                `${n.kind} · community ${community} · ${Math.round(n.degree)} connections`;
        } else {
            tooltip.style.display = "none";
        }
    });
    on(canvas, "mouseleave", () => {
        s.hovered = null;
        tooltip.style.display = "none";
        s.needsDraw = true;
    });
    on(canvas, "click", (ev) => {
        const n = pick(s, ev);
        if (n && onNodeClick) onNodeClick(JSON.stringify(n));
    });
    on(window, "resize", () => {
        resizeCanvas(s);
        s.needsDraw = true;
    });

    // Live re-color on theme flips (?theme= param and portfolio postMessage
    // both end up mutating data-theme on <html>).
    s.observer = new MutationObserver(() => {
        s.palette = readPalette();
        s.needsDraw = true;
    });
    s.observer.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["data-theme"],
    });

    const loop = () => {
        if (!state || state !== s) return;
        if (s.visible && s.needsDraw) {
            s.needsDraw = false;
            draw(s);
        }
        s.raf = requestAnimationFrame(loop);
    };
    s.raf = requestAnimationFrame(loop);
    return true;
};

window.__codeRagGraphDestroy = function () {
    const s = state;
    if (!s) return;
    state = null;
    cancelAnimationFrame(s.raf);
    if (s.sim) s.sim.stop();
    if (s.observer) s.observer.disconnect();
    for (const [target, type, fn] of s.listeners) target.removeEventListener(type, fn);
    if (s.tooltip && s.tooltip.parentNode) s.tooltip.parentNode.removeChild(s.tooltip);
};

window.__codeRagGraphResize = function () {
    const s = state;
    if (!s) return;
    resizeCanvas(s);
    s.needsDraw = true;
};

window.__codeRagGraphSetVisible = function (visible) {
    const s = state;
    if (!s) return;
    s.visible = !!visible;
    if (visible) s.needsDraw = true;
};

function resizeCanvas(s) {
    const rect = s.canvas.getBoundingClientRect();
    s.dpr = window.devicePixelRatio || 1;
    s.width = Math.max(rect.width, 1);
    s.height = Math.max(rect.height, 1);
    s.canvas.width = Math.round(s.width * s.dpr);
    s.canvas.height = Math.round(s.height * s.dpr);
}

/** Hit-test under the current zoom transform. */
function pick(s, ev) {
    const rect = s.canvas.getBoundingClientRect();
    const x = s.transform.invertX(ev.clientX - rect.left);
    const y = s.transform.invertY(ev.clientY - rect.top);
    let best = null;
    let bestDist = Infinity;
    for (const n of s.nodes) {
        const r = nodeRadius(n.degree, s.maxDegree) + 2;
        const dx = n.x - x;
        const dy = n.y - y;
        const d2 = dx * dx + dy * dy;
        if (d2 < r * r && d2 < bestDist) {
            best = n;
            bestDist = d2;
        }
    }
    return best;
}

function draw(s) {
    const { ctx, palette } = s;
    ctx.setTransform(s.dpr, 0, 0, s.dpr, 0, 0);
    ctx.clearRect(0, 0, s.width, s.height);
    ctx.translate(s.transform.x, s.transform.y);
    ctx.scale(s.transform.k, s.transform.k);

    // Edges first.
    for (const e of s.edges) {
        if (e.source.x == null || e.target.x == null) continue;
        ctx.globalAlpha = e.confidence === "extracted" ? ALPHA_EXTRACTED : ALPHA_INFERRED;
        ctx.strokeStyle = palette.edge;
        ctx.lineWidth = 1 / s.transform.k;
        ctx.setLineDash(dashFor(e.relations).map((d) => d / s.transform.k));
        ctx.beginPath();
        ctx.moveTo(e.source.x, e.source.y);
        ctx.lineTo(e.target.x, e.target.y);
        ctx.stroke();
    }
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;

    // Nodes, ringed with the surface color so overlaps stay separable.
    for (const n of s.nodes) {
        const r = nodeRadius(n.degree, s.maxDegree);
        ctx.beginPath();
        ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
        ctx.fillStyle = nodeColor(n, palette);
        ctx.fill();
        ctx.lineWidth = (n === s.hovered ? 2.5 : 1) / s.transform.k;
        ctx.strokeStyle = n === s.hovered ? palette.label : palette.surface;
        ctx.stroke();
    }

    // Labels above a zoom threshold, plus always for the top-degree nodes.
    const labelAll = s.transform.k >= 2.2;
    const topN = [...s.nodes]
        .sort((a, b) => (b.degree || 0) - (a.degree || 0))
        .slice(0, 15);
    const labeled = labelAll ? s.nodes : topN;
    ctx.fillStyle = palette.label;
    ctx.font = `${11 / s.transform.k}px Atkinson, sans-serif`;
    ctx.textAlign = "center";
    for (const n of labeled) {
        if (!n.label) continue;
        const r = nodeRadius(n.degree, s.maxDegree);
        ctx.fillText(n.label, n.x, n.y - r - 3 / s.transform.k);
    }
}
