/// A single critically-damped or under-damped spring track.
///
/// Integrates with a fixed sub-step of 1/480 s (matching the Flutter
/// SpringGroup), which makes it interruptible and framerate-independent.

const SUB_STEP: f32 = 1.0 / 480.0;

#[derive(Clone)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub epsilon: f32,
}

impl Spring {
    /// Create a spring using the Flutter-style *response / damping-ratio*
    /// parameterisation (mass = 1).
    pub fn new(response_s: f32, damping_ratio: f32, initial: f32) -> Self {
        use std::f32::consts::PI;
        let omega = 2.0 * PI / response_s;
        let stiffness = omega * omega;
        let damping = 2.0 * damping_ratio * omega;
        Spring {
            value: initial,
            velocity: 0.0,
            target: initial,
            stiffness,
            damping,
            epsilon: 0.1,
        }
    }

    pub fn with_epsilon(mut self, e: f32) -> Self {
        self.epsilon = e;
        self
    }

    pub fn reset(&mut self, value: f32) {
        self.value = value;
        self.velocity = 0.0;
        self.target = value;
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Step `dt` seconds.  Returns `true` while still moving.
    pub fn tick(&mut self, dt: f32) -> bool {
        let mut remaining = dt;
        while remaining > 0.0 {
            let step = remaining.min(SUB_STEP);
            remaining -= step;

            let force = -self.stiffness * (self.value - self.target)
                - self.damping * self.velocity;
            self.velocity += force * step;
            self.value += self.velocity * step;
        }
        !self.settled()
    }

    pub fn settled(&self) -> bool {
        (self.value - self.target).abs() < self.epsilon
            && self.velocity.abs() < self.epsilon
    }

    /// Force settlement at the target.
    pub fn finish(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }
}

/// Named spring profiles matching DbSpring.
pub mod profile {
    use super::Spring;

    fn make(response: f32, ratio: f32, initial: f32, epsilon: f32) -> Spring {
        Spring::new(response, ratio, initial).with_epsilon(epsilon)
    }

    pub fn arrival(initial: f32) -> Spring {
        make(0.52, 0.80, initial, 0.1)
    }
    pub fn departure(initial: f32) -> Spring {
        make(0.44, 1.0, initial, 0.1)
    }
    pub fn peek(initial: f32) -> Spring {
        make(0.34, 0.55, initial, 0.1)
    }
    pub fn atmosphere(initial: f32) -> Spring {
        make(0.70, 0.95, initial, 0.005)
    }
    pub fn reveal(initial: f32) -> Spring {
        make(0.34, 0.78, initial, 0.1)
    }
    pub fn withdraw(initial: f32) -> Spring {
        make(0.26, 1.0, initial, 0.1)
    }
    pub fn contour(initial: f32) -> Spring {
        make(0.60, 0.92, initial, 0.05)
    }
    pub fn content_in(initial: f32) -> Spring {
        make(0.62, 1.0, initial, 0.002)
    }
    pub fn content_out(initial: f32) -> Spring {
        make(0.26, 1.0, initial, 0.002)
    }
    pub fn for_direction(opening: bool, initial: f32) -> Spring {
        if opening { arrival(initial) } else { departure(initial) }
    }
}

/// A collection of named spring tracks that all tick together.
pub struct SpringGroup {
    pub width: Spring,
    pub height: Spring,
    pub radius: Spring,
    pub expansion: Spring,
    pub shadow: Spring,
    pub highlight: Spring,
}

impl SpringGroup {
    pub fn new(initial_w: f32, initial_h: f32, initial_r: f32) -> Self {
        SpringGroup {
            width: profile::arrival(initial_w),
            height: profile::arrival(initial_h),
            radius: profile::contour(initial_r),
            expansion: profile::content_in(0.0),
            shadow: profile::atmosphere(0.0),
            highlight: profile::atmosphere(0.0),
        }
    }

    /// Step `dt` seconds.  Returns `true` while still moving.
    pub fn tick(&mut self, dt: f32) -> bool {
        let a = self.width.tick(dt);
        let b = self.height.tick(dt);
        let c = self.radius.tick(dt);
        let d = self.expansion.tick(dt);
        let e = self.shadow.tick(dt);
        let f = self.highlight.tick(dt);
        a | b | c | d | e | f
    }

    pub fn all_settled(&self) -> bool {
        self.width.settled()
            && self.height.settled()
            && self.radius.settled()
            && self.expansion.settled()
            && self.shadow.settled()
            && self.highlight.settled()
    }
}
