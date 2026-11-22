/// Geometry constants mirrored from IslandMetrics / DbSize / DbRadius.
/// All values in logical pixels at scale = 1.0.

// ── Collapsed pill ──────────────────────────────────────────────────────────
pub const COLLAPSED_W: f32 = 160.0;
pub const COLLAPSED_H: f32 = 30.0;

// ── Hover-peek growth ───────────────────────────────────────────────────────
pub const PEEK_GROW_W: f32 = 28.0;
pub const PEEK_GROW_H: f32 = 6.0;

// ── Expanded pager ──────────────────────────────────────────────────────────
pub const EXPANDED_W: f32 = 486.0;
pub const EXPANDED_H: f32 = 348.0;

// ── Corner radii ────────────────────────────────────────────────────────────
/// Flutter keeps the top edge square because it sits under the physical notch.
/// On Windows the overlay floats on the desktop, so both top corners are
/// rounded to match the bottom capsule shape.
pub const RADIUS_TOP: f32 = 16.0;
pub const RADIUS_COLLAPSED_BOTTOM: f32 = 16.0;
pub const RADIUS_PEEK_BOTTOM: f32 = 19.0;
pub const RADIUS_EXPANDED_BOTTOM: f32 = 30.0;

// ── Stage ───────────────────────────────────────────────────────────────────
/// Fixed stage window size, in logical pixels. Must fit the largest expanded
/// island (486 × 348 at scale 1.35 => 656 × 470) with a little breathing room.
/// It must NOT be multiplied by monitor DPI again: iced/winit already treats
/// window sizes as logical pixels and scales the backing surface internally.
pub const STAGE_W: f32 = 720.0;
pub const STAGE_H: f32 = 520.0;

// ── Extra hit-test padding ───────────────────────────────────────────────────
pub const HIT_PAD_X: f32 = 10.0;
pub const HIT_PAD_Y: f32 = 6.0;

// ── Proximity zone (cursor watch) ───────────────────────────────────────────
pub const REVEAL_MARGIN_X: f32 = 130.0;
pub const REVEAL_MARGIN_Y: f32 = 20.0;

/// Island rect inside the stage (centred, anchored at y = 0).
pub fn island_rect(stage_w: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    let x = ((stage_w - w) / 2.0).max(0.0);
    (x, 0.0, w, h)
}

/// Hit rect: a bit larger than the island so the cursor is caught just before
/// it touches the shape.
pub fn hit_rect(x: f32, _y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    (x - HIT_PAD_X, 0.0, w + HIT_PAD_X * 2.0, h + HIT_PAD_Y)
}
