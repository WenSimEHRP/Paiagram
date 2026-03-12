# Graph View: Trip Follow Mode

## Overview

Add a "follow mode" to the Graph tab that allows users to click on a Trip arrow and have the viewport automatically track the vehicle as it moves along the railway network during time playback.

## Requirements

- Click a Trip arrow in Graph view to enter follow mode
- Viewport smoothly tracks the vehicle's position as time progresses
- Soft follow: any manual pan/zoom/Escape exits follow mode
- Clicking another Trip switches the follow target
- Vehicle stops at stations between arrival and departure times
- Follow mode auto-exits when the Trip's journey ends
- Auto-starts playback if GlobalTimer is paused when entering follow mode
- Followed Trip arrow is visually highlighted

## Design

### State Management

Extend `GraphNavigation` with a single field:

```rust
pub struct GraphNavigation {
    // ...existing fields...
    #[serde(skip)]
    following: Option<Entity>,  // Trip entity being followed
}
```

The field is `#[serde(skip)]` — follow state is ephemeral and must not be serialized into save files. No `MapEntities` handling needed since skipped fields won't contain stale entity references on load.

### Trip Position Query

The existing `TripSpatialIndex` queries by spatial+time range, not by entity. To find the followed Trip's position at a given time, query the Trip's `Children` + `EntryStop`/`EntryEstimate` components directly from the ECS:

1. Get the Trip's children (Entry entities) via Bevy's `Children` component
2. Find the two entries bracketing the current time (binary search by `EntryEstimate.dep` / `.arr`)
3. Interpolate position between their station positions using the existing time-linear formula

This avoids needing a new spatial index method and works regardless of whether the trip is currently visible in the viewport.

If the followed entity is despawned (e.g., user deletes the trip), the ECS query will fail — clear `following = None` gracefully.

### Follow Logic (per frame)

In `handle_navigation`, before normal navigation processing:

1. If `following` is `Some(trip_entity)`:
   - Query the Trip's interpolated position (see Trip Position Query above)
   - Smoothly move `x_offset` and `y_offset` toward centering the Trip position on screen, using `egui::emath::exponential_smooth_factor` for frame-rate-independent smoothing (consistent with existing keyboard navigation smoothing in `tabs.rs`)
   - If user input detected (drag, arrow keys, scroll zoom, Escape): set `following = None`
2. If `following` is `None`: existing navigation logic unchanged

### Interpolation

Use the existing time-linear interpolation (`pos0.lerp(pos1, f)` where `f = (current_time - t1) / (t2 - t1)`). Edges are currently straight line segments, so time-linear and distance-linear are equivalent. Can be upgraded to distance-based interpolation later if edges gain complex geometry.

### Station Stops

When the current time falls between an Entry's arrival and departure:
- The Trip's interpolated position is the station position (stationary)
- Viewport holds steady, centered on the station

### Trip Journey End & Repeat Mode

When `repeat_frequency` is disabled: if the current time exceeds the last Entry's departure time, set `following = None` (auto-exit) and playback continues.

When `repeat_frequency` is enabled: follow mode persists across repeat cycles. The follow logic must apply the same `rem_euclid(repeat_time)` wrapping to the current time before querying the Trip's position, consistent with existing rendering code.

### Interaction Entry Point

- Single-click on a Trip arrow in Graph view sets `following = Some(trip_entity)` and also selects the trip (additive — both follow and select)
- The click handler in `push_draw_items` returns a "start follow" signal (e.g., via a field on the return type or a local variable), which is acted on outside the immutable rendering path to mutate `GlobalTimer.animation_playing = true` if paused
- Reuse existing Trip click detection (spatial query on rendered Trip arrows)

### Exit Conditions

Follow mode exits (`following = None`) when:
- User drags to pan
- User presses arrow keys
- User scrolls to zoom
- User presses Escape
- Trip journey ends (without repeat mode)
- Followed Trip entity is despawned

Clicking another Trip does NOT exit — it switches the follow target.

### Visual Feedback

The followed Trip arrow is drawn with visual distinction (e.g., increased size, outline, or brighter color). Pass the `following` entity to `push_draw_items` alongside `selected_trips` so the renderer can apply distinct styling.

## Files Affected

- `crates/paiagram-ui/src/tabs/graph.rs` — GraphNavigation struct, handle_navigation, Trip click handling, push_draw_items rendering, follow signal plumbing
- `crates/paiagram-ui/src/tabs.rs` — Navigatable trait (if interface changes needed for follow state)
- `crates/paiagram-core/src/trip.rs` — Possibly add a helper to find bracketing entries for a Trip at a given time

## Out of Scope

- Distance-based interpolation along complex edge geometry (future enhancement)
- Follow mode in Diagram tab
- Multiple simultaneous follow targets
- Playback speed UI changes
