//! Clustered graph generation + CSR adjacency.
//!
//! Deterministic in `seed`. Positions are kept as `vec4` (xyz + community index
//! in `w`) so the buffer can be bound straight to the compute shader and reused
//! as a vertex buffer without a repack.

pub struct Rng(pub u32);

impl Rng {
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 16_777_216.0
    }
    #[inline]
    pub fn gauss(&mut self) -> f32 {
        let u = self.next_f32().max(1e-7);
        let v = self.next_f32();
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }
}

pub struct Graph {
    /// xyz + community index, 4 floats per node
    pub pos: Vec<f32>,
    /// edge endpoints, 2 indices per edge
    pub edges: Vec<u32>,
    /// CSR row offsets, len = nodes + 1
    pub csr_off: Vec<u32>,
    /// CSR neighbour list, len = 2 * edges
    pub csr_nbr: Vec<u32>,
    /// community centres, 4 floats each
    pub centroids: Vec<f32>,
    pub nodes: usize,
    pub clusters: usize,
}

impl Graph {
    pub fn generate(nodes: usize, clusters: usize, density: f32, seed: u32) -> Graph {
        let clusters = clusters.max(1);
        let mut rng = Rng(seed | 1);

        let mut centroids = vec![0.0f32; clusters * 4];
        for c in 0..clusters {
            let theta = rng.next_f32() * std::f32::consts::TAU;
            let phi = (2.0 * rng.next_f32() - 1.0).acos();
            let r = 0.45 + 0.55 * rng.next_f32().cbrt();
            centroids[c * 4] = r * phi.sin() * theta.cos();
            centroids[c * 4 + 1] = r * phi.sin() * theta.sin() * 0.72;
            centroids[c * 4 + 2] = r * phi.cos();
        }

        let mut pos = vec![0.0f32; nodes * 4];
        let mut members: Vec<Vec<u32>> = vec![Vec::with_capacity(nodes / clusters + 8); clusters];
        for i in 0..nodes {
            let c = (rng.next_f32() * clusters as f32) as usize % clusters;
            let sd = 0.055 + 0.07 * rng.next_f32();
            pos[i * 4] = centroids[c * 4] + rng.gauss() * sd;
            pos[i * 4 + 1] = centroids[c * 4 + 1] + rng.gauss() * sd * 0.85;
            pos[i * 4 + 2] = centroids[c * 4 + 2] + rng.gauss() * sd;
            pos[i * 4 + 3] = c as f32;
            members[c].push(i as u32);
        }

        let extra = (density * 1.2).round().clamp(0.0, 4.0) as usize;
        let mut edges: Vec<u32> = Vec::with_capacity(nodes * (2 + extra));
        for c in 0..clusters {
            let m = &members[c];
            if m.is_empty() {
                continue;
            }
            for (k, &a) in m.iter().enumerate() {
                let hop = 1 + (rng.next_f32() * 9.0) as usize;
                let b = m[(k + hop) % m.len()];
                if a != b {
                    edges.push(a);
                    edges.push(b);
                }
                for _ in 0..extra {
                    if rng.next_f32() > 0.22 * density {
                        continue;
                    }
                    let d = m[(rng.next_f32() * m.len() as f32) as usize % m.len()];
                    if d != a {
                        edges.push(a);
                        edges.push(d);
                    }
                }
            }
            for _ in 0..5 {
                let o = &members[(rng.next_f32() * clusters as f32) as usize % clusters];
                if !o.is_empty() {
                    edges.push(m[(rng.next_f32() * m.len() as f32) as usize % m.len()]);
                    edges.push(o[(rng.next_f32() * o.len() as f32) as usize % o.len()]);
                }
            }
        }

        let (csr_off, csr_nbr) = Self::build_csr(&edges, nodes);
        Graph { pos, edges, csr_off, csr_nbr, centroids, nodes, clusters }
    }

    /// Counting sort into CSR: one pass for degrees, one prefix sum, one scatter.
    fn build_csr(edges: &[u32], nodes: usize) -> (Vec<u32>, Vec<u32>) {
        let mut off = vec![0u32; nodes + 1];
        for &v in edges {
            off[v as usize + 1] += 1;
        }
        for i in 0..nodes {
            off[i + 1] += off[i];
        }
        let mut nbr = vec![0u32; off[nodes] as usize];
        let mut cursor = off.clone();
        for e in edges.chunks_exact(2) {
            let (a, b) = (e[0] as usize, e[1] as usize);
            nbr[cursor[a] as usize] = b as u32;
            cursor[a] += 1;
            nbr[cursor[b] as usize] = a as u32;
            cursor[b] += 1;
        }
        (off, nbr)
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len() / 2
    }
}
