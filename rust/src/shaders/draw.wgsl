struct U { rot : mat4x4f, fix : vec2f, scale : f32, alpha : f32, pt : vec2f };
@group(0) @binding(0) var<uniform> u : U;

struct VOut {
  @builtin(position) pos : vec4f,
  @location(0) uv : vec2f,
  @location(1) d  : f32,
};

@vertex fn vs_node(@location(0) corner : vec2f, @location(1) p : vec3f) -> VOut {
  let r = (u.rot * vec4f(p, 1.0)).xyz;
  let persp = 1.0 / (2.0 - r.z * 0.45);
  let size = u.pt * (0.75 + 0.6 * (r.z + 1.0));
  var o : VOut;
  o.pos = vec4f(r.xy * u.scale * persp * u.fix + corner * size, 0.0, 1.0);
  o.uv = corner;
  o.d = r.z;
  return o;
}

@fragment fn fs_node(i : VOut) -> @location(0) vec4f {
  if (dot(i.uv, i.uv) > 0.25) { discard; }
  let f = 0.28 + 0.72 * smoothstep(-1.1, 1.0, i.d);
  return vec4f(1.0, 1.0, 1.0, u.alpha * f);
}

@vertex fn vs_edge(@location(0) p : vec3f) -> VOut {
  let r = (u.rot * vec4f(p, 1.0)).xyz;
  let persp = 1.0 / (2.0 - r.z * 0.45);
  var o : VOut;
  o.pos = vec4f(r.xy * u.scale * persp * u.fix, 0.0, 1.0);
  o.uv = vec2f(0.0);
  o.d = r.z;
  return o;
}

@fragment fn fs_edge(i : VOut) -> @location(0) vec4f {
  let f = 0.2 + 0.8 * smoothstep(-1.1, 1.0, i.d);
  return vec4f(0.86, 0.93, 0.96, u.alpha * f);
}
