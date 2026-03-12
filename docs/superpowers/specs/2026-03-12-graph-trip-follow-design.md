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
    following: Option<Entity>,  // Trip entity being followed
}
```

No new Resources, Components, or Systems needed.

### Follow Logic (per frame)

In `handle_navigation`, before normal navigation processing:

1. If `following` is `Some(trip_entity)`:
   - Query the Trip's interpolated position at the current `GlobalTimer` tick (reuse existing Trip spatial query / interpolation logic)
   - Smoothly lerp `x_offset` and `y_offset` toward centering the Trip position on screen
   - If user input detected (drag, arrow keys, scroll zoom, Escape): set `following = None`
2. If `following` is `None`: existing navigation logic unchanged

### Interpolation

Use the existing time-linear interpolation (`pos0.lerp(pos1, f)` where `f = (current_time - t1) / (t2 - t1)`). Edges are currently straight line segments, so time-linear and distance-linear are equivalent. Can be upgraded to distance-based interpolation later if edges gain complex geometry.

### Station Stops

When the current time falls between an Entry's arrival and departure:
- The Trip's interpolated position is the station position (stationary)
- Viewport holds steady, centered on the station

### Trip Journey End

When the current time exceeds the last Entry's departure time:
- Set `following = None` (auto-exit follow mode)
- Playback continues (user can still observe other Trips)

### Interaction Entry Point

- Single-click on a Trip arrow in Graph view sets `following = Some(trip_entity)`
- If `GlobalTimer.animation_playing` is false, set it to true
- Reuse existing Trip click detection (spatial query on rendered Trip arrows)

### Exit Conditions

Follow mode exits (`following = None`) when:
- User drags to pan
- User presses arrow keys
- User scrolls to zoom
- User presses Escape
- Trip journey ends (current time > last departure)

Clicking another Trip does NOT exit — it switches the follow target.

### Visual Feedback

The followed Trip arrow is drawn with visual distinction (e.g., increased size, outline, or brighter color) so the user can identify which Trip is being tracked.

## Files Affected

- `crates/paiagram-ui/src/tabs/graph.rs` — GraphNavigation struct, handle_navigation, Trip click handling, Trip rendering highlight
- `crates/paiagram-ui/src/tabs.rs` — Navigatable trait (if interface changes needed for follow state)

## Out of Scope

- Distance-based interpolation along complex edge geometry (future enhancement)
- Follow mode in Diagram tab
- Multiple simultaneous follow targets
- Playback speed UI changes
