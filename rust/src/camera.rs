//! Camera motion, ported verbatim from the page's rAF loop.
//!
//! Deliberately free of any browser API: it takes `performance.now()` as a
//! number and returns a rotation and scale. That keeps it testable natively and
//! makes it the single definition of the hero's motion, rather than one copy per
//! renderer.

/// A column-major 3x3 rotation plus the zoom the frame should use.
pub struct View {
    pub rot: [f32; 9],
    pub scale: f32,
}

pub struct Camera {
    // tunables — the page's prop defaults
    pub drift_speed: f32,
    pub parallax: f32,
    pub mass: f32,
    pub zoom: f32,

    // inputs, driven from pointer or device tilt; both in -1..1
    pub mx: f32,
    pub my: f32,
    /// Device tilt is a direct 1:1 look-around: no spring, no smoothing.
    pub tilting: bool,
    pub dolly_target: f32,

    // integrator state
    pmx: f32,
    pmy: f32,
    vx: f32,
    vy: f32,
    dolly: f32,
    dv: f32,
    t0: f64,
}

impl Camera {
    /// The page's component props, which are NOT the `??` fallbacks in its
    /// source — the dc runtime supplies its own declared defaults (zoom 4).
    pub fn set_props(&mut self, zoom: f32, drift_speed: f32, parallax: f32, mass: f32) {
        self.zoom = zoom;
        self.drift_speed = drift_speed;
        self.parallax = parallax;
        self.mass = mass.max(0.05);
    }

    pub fn new(now_ms: f64) -> Camera {
        Camera {
            drift_speed: 0.3,
            parallax: 0.07,
            mass: 1.0,
            zoom: 1.7,
            mx: 0.0,
            my: 0.0,
            tilting: false,
            dolly_target: 1.0,
            pmx: 0.0,
            pmy: 0.0,
            vx: 0.0,
            vy: 0.0,
            dolly: 1.0,
            dv: 0.0,
            t0: now_ms,
        }
    }

    /// Wheel dolly, clamped to the same depth range the page used.
    pub fn nudge_dolly(&mut self, factor: f32) {
        self.dolly_target = (self.dolly_target * factor).clamp(1.0, 1.45);
    }

    /// Advance one frame. `now_ms` is `performance.now()`.
    pub fn step(&mut self, now_ms: f64) -> View {
        let t = (now_ms - self.t0) as f32 / 1000.0 * 0.055 * self.drift_speed;

        // Pointer parallax as a heavy spring-damper: the mass accelerates slowly,
        // keeps moving after the cursor stops, and settles without snapping.
        let k = 0.0016 / self.mass;
        let damp = 1.0 - 0.028 * (1.0 / self.mass);
        if self.tilting {
            self.pmx = self.mx;
            self.pmy = self.my;
            self.vx = 0.0;
            self.vy = 0.0;
        } else {
            self.vx += (self.mx - self.pmx) * k;
            self.vy += (self.my - self.pmy) * k;
            self.vx *= damp;
            self.vy *= damp;
            self.pmx += self.vx;
            self.pmy += self.vy;
        }

        let look = if self.tilting { 4.5 } else { 1.0 };
        let yaw = t - self.pmx * self.parallax * 0.9 * look;
        let (sa, ca) = yaw.sin_cos();
        let tilt = 0.42 + (t * 0.55).sin() * 0.12 - self.pmy * self.parallax * 0.7 * look;

        // dolly carries the same mass as the parallax
        self.dv += (self.dolly_target - self.dolly) * 0.0055 / self.mass;
        self.dv *= 1.0 - 0.045 * (1.0 / self.mass);
        self.dolly += self.dv;
        let (sb, cb) = tilt.sin_cos();

        View {
            // rotate about Y then tilt about X (column-major mat3)
            rot: [ca, sb * sa, -cb * sa, 0.0, cb, sb, sa, -sb * ca, cb * ca],
            scale: self.zoom * 1.55 * self.dolly,
        }
    }
}
