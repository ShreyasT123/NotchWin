/// Island intent / phase / mode — matching the Flutter state machine exactly.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intent {
    Hidden,
    Collapsed,
    Expanded,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Hidden,
    Collapsed,
    Expanding,
    Expanded,
    Interactive,
    Collapsing,
}

impl Phase {
    pub fn is_open(self) -> bool {
        matches!(self, Phase::Expanded | Phase::Interactive)
    }
    pub fn is_visible(self) -> bool {
        !matches!(self, Phase::Hidden)
    }
}

/// Derives the phase from intent + animation progress + velocity.
/// Mirrors `IslandController._derivePhase`.
pub fn derive_phase(
    intent: Intent,
    progress: f32,
    velocity: f32,
    focused: bool,
) -> Phase {
    const EPSILON: f32 = 0.02;

    if intent == Intent::Hidden && progress <= EPSILON {
        return Phase::Hidden;
    }
    if progress >= 1.0 - EPSILON {
        return if focused { Phase::Interactive } else { Phase::Expanded };
    }
    if progress <= EPSILON {
        return if intent == Intent::Hidden {
            Phase::Hidden
        } else {
            Phase::Collapsed
        };
    }
    if velocity > 0.0 {
        return Phase::Expanding;
    }
    if velocity < 0.0 {
        return Phase::Collapsing;
    }
    if intent == Intent::Expanded {
        Phase::Expanding
    } else {
        Phase::Collapsing
    }
}

/// Island geometry for a given intent.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub width: f32,
    pub height: f32,
    pub bottom_radius: f32,
    pub expansion: f32,
    pub shadow: f32,
}

pub fn resolve_geometry(
    intent: Intent,
    hovered: bool,
) -> Geometry {
    use crate::metrics::*;
    match intent {
        Intent::Hidden => Geometry {
            width: COLLAPSED_W * 0.72,
            height: 0.0,
            bottom_radius: RADIUS_COLLAPSED_BOTTOM,
            expansion: 0.0,
            shadow: 0.0,
        },
        Intent::Collapsed => {
            if hovered {
                Geometry {
                    width: COLLAPSED_W + PEEK_GROW_W,
                    height: COLLAPSED_H + PEEK_GROW_H,
                    bottom_radius: RADIUS_PEEK_BOTTOM,
                    expansion: 0.0,
                    shadow: 0.45,
                }
            } else {
                Geometry {
                    width: COLLAPSED_W,
                    height: COLLAPSED_H,
                    bottom_radius: RADIUS_COLLAPSED_BOTTOM,
                    expansion: 0.0,
                    shadow: 0.2,
                }
            }
        }
        Intent::Expanded => Geometry {
            width: EXPANDED_W,
            height: EXPANDED_H,
            bottom_radius: RADIUS_EXPANDED_BOTTOM,
            expansion: 1.0,
            shadow: 1.0,
        },
    }
}
