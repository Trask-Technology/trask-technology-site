// WebGPU renderer + GPU-resident force layout for the hero graph.
//
// The graph never leaves the GPU: positions live in a storage buffer that a
// compute pass integrates each frame and the render pass reads directly as a
// vertex buffer. Nothing is copied back to the CPU.
//
// Returns null when WebGPU is unavailable so the caller can fall back to WebGL2.

const UNIFORM_SIZE = 96;  // mat4x4(64) + vec2 fix(8) + scale(4) + alpha(4) + pt vec2(8) + pad
const SIM_SIZE = 48;      // 6 x u32/f32 padded
const WG = 64;            // compute workgroup size
const MAX_DEG = 24;       // neighbours considered per node per step

const DRAW_SHADER = /* wgsl */ `
struct U { rot : mat4x4f, fix : vec2f, scale : f32, alpha : f32, pt : vec2f };
@group(0) @binding(0) var<uniform> u : U;

struct VOut {
  @builtin(position) pos : vec4f,
  @location(0) uv : vec2f,
  @location(1) d  : f32,
};

@vertex fn vsNode(@location(0) corner : vec2f, @location(1) p : vec3f) -> VOut {
  let r = (u.rot * vec4f(p, 1.0)).xyz;
  let persp = 1.0 / (2.0 - r.z * 0.45);
  let size = u.pt * (0.75 + 0.6 * (r.z + 1.0));
  var o : VOut;
  o.pos = vec4f(r.xy * u.scale * persp * u.fix + corner * size, 0.0, 1.0);
  o.uv = corner;
  o.d = r.z;
  return o;
}

@fragment fn fsNode(i : VOut) -> @location(0) vec4f {
  if (dot(i.uv, i.uv) > 0.25) { discard; }
  let f = 0.28 + 0.72 * smoothstep(-1.1, 1.0, i.d);
  return vec4f(1.0, 1.0, 1.0, u.alpha * f);
}

@vertex fn vsEdge(@location(0) p : vec3f) -> VOut {
  let r = (u.rot * vec4f(p, 1.0)).xyz;
  let persp = 1.0 / (2.0 - r.z * 0.45);
  var o : VOut;
  o.pos = vec4f(r.xy * u.scale * persp * u.fix, 0.0, 1.0);
  o.uv = vec2f(0.0);
  o.d = r.z;
  return o;
}

@fragment fn fsEdge(i : VOut) -> @location(0) vec4f {
  let f = 0.2 + 0.8 * smoothstep(-1.1, 1.0, i.d);
  return vec4f(0.86, 0.93, 0.96, u.alpha * f);
}
`;

// One invocation per node: spring attraction along its CSR neighbour list,
// repulsion from cluster centroids, pull toward its own community, integrate.
const SIM_SHADER = /* wgsl */ `
struct Sim {
  n        : u32,
  clusters : u32,
  dt       : f32,
  spring   : f32,
  repel    : f32,
  damp     : f32,
};
@group(0) @binding(0) var<storage, read_write> pos : array<vec4f>;
@group(0) @binding(1) var<storage, read_write> vel : array<vec4f>;
@group(0) @binding(2) var<storage, read>      off : array<u32>;
@group(0) @binding(3) var<storage, read>      nbr : array<u32>;
@group(0) @binding(4) var<storage, read>      cen : array<vec4f>;
@group(0) @binding(5) var<uniform>            s   : Sim;
@group(0) @binding(6) var<storage, read>      rest : array<vec4f>;

@compute @workgroup_size(${WG})
fn step(@builtin(global_invocation_id) gid : vec3u) {
  let i = gid.x;
  if (i >= s.n) { return; }

  let me = pos[i];
  let p = me.xyz;
  let owner = u32(me.w);

  var f = vec3f(0.0);

  // springs — bounded degree keeps the step cost predictable
  let a = off[i];
  let b = min(off[i + 1u], a + ${MAX_DEG}u);
  for (var k = a; k < b; k = k + 1u) {
    f = f + (pos[nbr[k]].xyz - p) * s.spring;
  }

  // community repulsion: O(clusters), not O(n)
  for (var c = 0u; c < s.clusters; c = c + 1u) {
    let d = p - cen[c].xyz;
    f = f + d * (s.repel / max(dot(d, d), 0.015));
  }

  // stay attached to your own community
  // anchor: the generated layout is the rest state, so the sim animates
  // around it instead of collapsing inward or drifting off screen
  f = f + (rest[i].xyz - p) * 2.6;

  var v = (vel[i].xyz + f * s.dt) * s.damp;
  var np = p + v * s.dt;
  let r = length(np);
  if (r > 1.7) { np = np * (1.7 / r); v = v * 0.4; }

  vel[i] = vec4f(v, 0.0);
  pos[i] = vec4f(np, me.w);
}
`;

export async function createGPURenderer(canvas, initialGraph) {
  if (!canvas || !navigator.gpu) return null;
  const adapter = await navigator.gpu.requestAdapter({ powerPreference: "high-performance" });
  if (!adapter) return null;

  // ask for enough storage to hold the biggest graph the slider allows
  const maxBuf = Math.min(adapter.limits.maxStorageBufferBindingSize, 1 << 29);
  const device = await adapter.requestDevice({
    requiredLimits: {
      maxStorageBufferBindingSize: maxBuf,
      maxBufferSize: Math.min(adapter.limits.maxBufferSize, 1 << 30),
    },
  });
  const ctx = canvas.getContext("webgpu");
  if (!ctx) return null;

  const format = navigator.gpu.getPreferredCanvasFormat();
  ctx.configure({ device, format, alphaMode: "opaque" });

  let lost = false;
  device.lost.then(() => { lost = true; });

  const drawModule = device.createShaderModule({ code: DRAW_SHADER });
  const simModule = device.createShaderModule({ code: SIM_SHADER });
  const blend = {
    color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha", operation: "add" },
    alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha", operation: "add" },
  };

  const quad = new Float32Array([-0.5, -0.5, 0.5, -0.5, 0.5, 0.5, -0.5, -0.5, 0.5, 0.5, -0.5, 0.5]);
  const quadBuf = device.createBuffer({ size: quad.byteLength, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
  device.queue.writeBuffer(quadBuf, 0, quad);

  const mkUniform = size => {
    const buffer = device.createBuffer({ size, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
    return { buffer, data: new Float32Array(size / 4) };
  };
  const uNode = mkUniform(UNIFORM_SIZE);
  const uEdge = mkUniform(UNIFORM_SIZE);
  const uSim = device.createBuffer({ size: SIM_SIZE, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const simData = new ArrayBuffer(SIM_SIZE);
  const simU32 = new Uint32Array(simData);
  const simF32 = new Float32Array(simData);

  const nodePipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: {
      module: drawModule, entryPoint: "vsNode",
      buffers: [
        { arrayStride: 8, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x2" }] },
        { arrayStride: 16, stepMode: "instance", attributes: [{ shaderLocation: 1, offset: 0, format: "float32x3" }] },
      ],
    },
    fragment: { module: drawModule, entryPoint: "fsNode", targets: [{ format, blend }] },
    primitive: { topology: "triangle-list" },
  });

  const edgePipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: {
      module: drawModule, entryPoint: "vsEdge",
      buffers: [{ arrayStride: 16, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }] }],
    },
    fragment: { module: drawModule, entryPoint: "fsEdge", targets: [{ format, blend }] },
    primitive: { topology: "line-list" },
  });

  device.pushErrorScope("validation");
  const simPipeline = device.createComputePipeline({
    layout: "auto",
    compute: { module: simModule, entryPoint: "step" },
  });
  const simError = await device.popErrorScope();
  if (simError) {
    console.warn("compute layout unavailable:", simError.message);
    return null;   // let the caller fall back to WebGL2 rather than show a dead canvas
  }

  const nodeBind = device.createBindGroup({ layout: nodePipeline.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: uNode.buffer } }] });
  const edgeBind = device.createBindGroup({ layout: edgePipeline.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: uEdge.buffer } }] });

  let graph, posBuf, velBuf, restBuf, offBuf, nbrBuf, idxBuf, cenBuf, simBind, groups;

  function uploadGraph(g) {
    for (const b of [posBuf, velBuf, restBuf, offBuf, nbrBuf, idxBuf, cenBuf]) b?.destroy?.();
    graph = g;

    // positions as vec4: xyz + community index in w
    const pos4 = new Float32Array(g.n * 4);
    for (let i = 0; i < g.n; i++) {
      pos4[i * 4] = g.pos[i * 3];
      pos4[i * 4 + 1] = g.pos[i * 3 + 1];
      pos4[i * 4 + 2] = g.pos[i * 3 + 2];
      pos4[i * 4 + 3] = g.owner[i];
    }

    const store = GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST;
    posBuf = device.createBuffer({ size: pos4.byteLength, usage: store | GPUBufferUsage.VERTEX });
    device.queue.writeBuffer(posBuf, 0, pos4);
    velBuf = device.createBuffer({ size: pos4.byteLength, usage: store });
    restBuf = device.createBuffer({ size: pos4.byteLength, usage: store | GPUBufferUsage.COPY_DST });
    device.queue.writeBuffer(restBuf, 0, pos4);
    offBuf = device.createBuffer({ size: g.csrOff.byteLength, usage: store });
    device.queue.writeBuffer(offBuf, 0, g.csrOff);
    nbrBuf = device.createBuffer({ size: Math.max(4, g.csrNbr.byteLength), usage: store });
    device.queue.writeBuffer(nbrBuf, 0, g.csrNbr);
    cenBuf = device.createBuffer({ size: g.centroids.byteLength, usage: store });
    device.queue.writeBuffer(cenBuf, 0, g.centroids);
    idxBuf = device.createBuffer({ size: Math.ceil(g.idx.byteLength / 4) * 4, usage: GPUBufferUsage.INDEX | GPUBufferUsage.COPY_DST });
    device.queue.writeBuffer(idxBuf, 0, g.idx);

    simBind = device.createBindGroup({
      layout: simPipeline.getBindGroupLayout(0),
      entries: [
        { binding: 0, resource: { buffer: posBuf } },
        { binding: 1, resource: { buffer: velBuf } },
        { binding: 2, resource: { buffer: offBuf } },
        { binding: 3, resource: { buffer: nbrBuf } },
        { binding: 4, resource: { buffer: cenBuf } },
        { binding: 5, resource: { buffer: uSim } },
        { binding: 6, resource: { buffer: restBuf } },
      ],
    });
    groups = Math.ceil(g.n / WG);
  }
  uploadGraph(initialGraph);

  const clear = { r: 0.0824, g: 0.3098, b: 0.4118, a: 1 };

  function resize(dpr = Math.min(devicePixelRatio || 1, 1.5)) {
    const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width === w && canvas.height === h) return;
    canvas.width = w; canvas.height = h;
    // resizing invalidates the swap chain; without this the stale surface
    // shows as an uncleared band after an orientation change
    ctx.configure({ device, format, alphaMode: "opaque" });
  }

  function writeUniform(u, rot3, fix, scale, alpha, ptx, pty) {
    const d = u.data;
    d[0] = rot3[0]; d[1] = rot3[1]; d[2] = rot3[2]; d[3] = 0;
    d[4] = rot3[3]; d[5] = rot3[4]; d[6] = rot3[5]; d[7] = 0;
    d[8] = rot3[6]; d[9] = rot3[7]; d[10] = rot3[8]; d[11] = 0;
    d[12] = 0; d[13] = 0; d[14] = 0; d[15] = 1;
    d[16] = fix[0]; d[17] = fix[1]; d[18] = scale; d[19] = alpha;
    d[20] = ptx; d[21] = pty;
    device.queue.writeBuffer(u.buffer, 0, d);
  }

  // sim params, tunable from the page
  const sim = { dt: 0.016, spring: 0.6, repel: 0.02, damp: 0.9, on: true };

  function frame(rot3, fix, scale, pointPx, nodeAlpha = 0.62, edgeAlpha = 0.085) {
    if (lost) return;
    resize();
    const ptx = (pointPx * 2) / canvas.width;
    const pty = (pointPx * 2) / canvas.height;
    writeUniform(uEdge, rot3, fix, scale, edgeAlpha, 0, 0);
    writeUniform(uNode, rot3, fix, scale, nodeAlpha, ptx, pty);

    simU32[0] = graph.n; simU32[1] = graph.clusters;
    simF32[2] = sim.dt; simF32[3] = sim.spring; simF32[4] = sim.repel; simF32[5] = sim.damp;
    device.queue.writeBuffer(uSim, 0, simData);

    const enc = device.createCommandEncoder();

    if (sim.on) {
      const cp = enc.beginComputePass();
      cp.setPipeline(simPipeline);
      cp.setBindGroup(0, simBind);
      cp.dispatchWorkgroups(groups);
      cp.end();
    }

    const pass = enc.beginRenderPass({
      colorAttachments: [{ view: ctx.getCurrentTexture().createView(), clearValue: clear, loadOp: "clear", storeOp: "store" }],
    });
    pass.setPipeline(edgePipeline);
    pass.setBindGroup(0, edgeBind);
    pass.setVertexBuffer(0, posBuf);
    pass.setIndexBuffer(idxBuf, "uint32");
    pass.drawIndexed(graph.idx.length);

    pass.setPipeline(nodePipeline);
    pass.setBindGroup(0, nodeBind);
    pass.setVertexBuffer(0, quadBuf);
    pass.setVertexBuffer(1, posBuf);
    pass.draw(6, graph.n);
    pass.end();

    device.queue.submit([enc.finish()]);
  }

  return {
    api: "webgpu",
    computeLayout: true,
    frame,
    resize,
    setGraph: uploadGraph,
    sim,
    get lost() { return lost; },
    destroy() { lost = true; device.destroy?.(); },
  };
}
