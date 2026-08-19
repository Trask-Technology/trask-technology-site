// One invocation per node. Springs along the node's CSR neighbour slice,
// repulsion from community centroids (O(clusters), not O(n)), then integrate.
// Positions stay in GPU memory for the renderer to read as vertices.

struct Sim {
  n        : u32,
  clusters : u32,
  dt       : f32,
  spring   : f32,
  repel    : f32,
  damp     : f32,
  max_deg  : u32,
  _pad     : u32,
};

@group(0) @binding(0) var<storage, read_write> pos : array<vec4f>;
@group(0) @binding(1) var<storage, read_write> vel : array<vec4f>;
@group(0) @binding(2) var<storage, read>      off : array<u32>;
@group(0) @binding(3) var<storage, read>      nbr : array<u32>;
@group(0) @binding(4) var<storage, read>      cen : array<vec4f>;
@group(0) @binding(5) var<uniform>            s   : Sim;
@group(0) @binding(6) var<storage, read>      rest : array<vec4f>;

@compute @workgroup_size(64)
fn step(@builtin(global_invocation_id) gid : vec3u) {
  let i = gid.x;
  if (i >= s.n) { return; }

  let me = pos[i];
  let p = me.xyz;
  let owner = u32(me.w);

  var f = vec3f(0.0);

  let a = off[i];
  let b = min(off[i + 1u], a + s.max_deg);
  for (var k = a; k < b; k = k + 1u) {
    f = f + (pos[nbr[k]].xyz - p) * s.spring;
  }

  for (var c = 0u; c < s.clusters; c = c + 1u) {
    let d = p - cen[c].xyz;
    f = f + d * (s.repel / max(dot(d, d), 0.015));
  }

  f = f + (cen[owner].xyz - p) * 0.45;

  // the generated layout is the rest state: the sim animates around it rather
  // than collapsing into the centroids or drifting off screen
  f = f + (rest[i].xyz - p) * 2.6;

  var v = (vel[i].xyz + f * s.dt) * s.damp;
  var np = p + v * s.dt;
  let r = length(np);
  if (r > 1.7) {
    np = np * (1.7 / r);
    v = v * 0.4;
  }

  vel[i] = vec4f(v, 0.0);
  pos[i] = vec4f(np, me.w);
}
