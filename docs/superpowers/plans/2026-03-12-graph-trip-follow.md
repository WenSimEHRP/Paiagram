# Graph View Trip Follow Mode — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow users to click a Trip arrow in Graph view and have the viewport smoothly follow the vehicle as time plays.

**Architecture:** Extend `GraphNavigation` with an `Option<Entity>` follow target. Add a helper on `TripSpatialIndex` to locate a specific Trip's segment at a given time. The `display()` function handles follow viewport updates, click-to-follow, and visual highlighting.

**Tech Stack:** Rust, Bevy ECS, egui, rstar (RTree)

**Spec:** `docs/superpowers/specs/2026-03-12-graph-trip-follow-design.md`

---

## Chunk 1: State & Query Infrastructure

### Task 1: Add `following` field to `GraphNavigation`

**Files:**
- Modify: `crates/paiagram-ui/src/tabs/graph.rs:89-106`

- [ ] **Step 1: Add the field**

In the `GraphNavigation` struct, add the `following` field with `#[serde(skip)]`:

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct GraphNavigation {
    x_offset: f64,
    y_offset: f64,
    zoom: f32,
    visible: egui::Rect,
    #[serde(skip)]
    following: Option<Entity>,
}
```

Update `Default` impl to include `following: None`.

- [ ] **Step 2: Add accessor methods**

Add methods on `GraphNavigation`:

```rust
impl GraphNavigation {
    pub fn following(&self) -> Option<Entity> {
        self.following
    }
    pub fn set_following(&mut self, entity: Option<Entity>) {
        self.following = entity;
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo clippy --workspace`
Expected: No new errors or warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/paiagram-ui/src/tabs/graph.rs
git commit -m "feat(graph): add following field to GraphNavigation"
```

---

### Task 2: Add trip position query helper on `TripSpatialIndex`

**Files:**
- Modify: `crates/paiagram-core/src/trip.rs:79-106`

- [ ] **Step 1: Add `query_trip_at_time` method**

Add a method that finds the `TripSpatialIndexItem` for a specific trip at a given time. This scans the time slice of the RTree and filters by entity:

```rust
impl TripSpatialIndex {
    /// Find the spatial index item for a specific trip at a given time.
    /// Returns the matching segment if the trip is active at that time.
    pub fn query_trip_at_time(&self, trip: Entity, time: f64) -> Option<TripSpatialIndexItem> {
        // Query with full spatial range but narrow time range
        let envelope = AABB::from_corners(
            [f64::NEG_INFINITY, f64::NEG_INFINITY, time],
            [f64::INFINITY, f64::INFINITY, time],
        );
        self.tree
            .locate_in_envelope_intersecting(&envelope)
            .find(|item| item.trip == trip)
            .copied()
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo clippy --workspace`
Expected: No new errors or warnings.

- [ ] **Step 3: Commit**

```bash
git add crates/paiagram-core/src/trip.rs
git commit -m "feat(trip): add query_trip_at_time to TripSpatialIndex"
```

---

## Chunk 2: Follow Viewport Logic

### Task 3: Implement follow viewport tracking in `display()`

**Files:**
- Modify: `crates/paiagram-ui/src/tabs/graph.rs:222-408` (the `display()` function)

- [ ] **Step 1: Add follow viewport update before `handle_navigation`**

After `tab.navi.visible = response.rect;` (line 225) and before `tab.navi.handle_navigation(ui, &response);` (line 226), add follow tracking logic:

```rust
tab.navi.visible = response.rect;

// Follow mode: update viewport to track the followed trip
let mut follow_active = false;
if let Some(trip_entity) = tab.navi.following {
    // Check entity still exists (handles despawn before async index rebuild)
    if world.get_entity(trip_entity).is_err() {
        tab.navi.following = None;
    }
}
if let Some(trip_entity) = tab.navi.following {
    let timer = world.resource::<GlobalTimer>();
    let time = timer.read_seconds();
    let settings = world.resource::<ProjectSettings>();
    let repeat_time = settings.repeat_frequency.0 as f64;
    let query_time = if repeat_time > 0.0 {
        time.rem_euclid(repeat_time)
    } else {
        time
    };
    let trip_spatial_index = world.resource::<TripSpatialIndex>();
    if let Some(sample) = trip_spatial_index.query_trip_at_time(trip_entity, query_time) {
        // Interpolate trip position
        let pos = if query_time <= sample.t1 {
            sample.p0
        } else if query_time >= sample.t2 {
            sample.p1
        } else {
            let f = (query_time - sample.t1) / (sample.t2 - sample.t1).max(f64::EPSILON);
            [
                sample.p0[0] + (sample.p1[0] - sample.p0[0]) * f,
                sample.p0[1] + (sample.p1[1] - sample.p0[1]) * f,
            ]
        };
        // Smooth viewport centering using exponential smoothing
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        let t = egui::emath::exponential_smooth_factor(0.9, 0.3, dt);
        let view_width = response.rect.width() as f64 / tab.navi.zoom as f64;
        let view_height = response.rect.height() as f64 / tab.navi.zoom as f64;
        let target_x = pos[0] - view_width / 2.0;
        let target_y = pos[1] - view_height / 2.0;
        let new_x = tab.navi.x_offset + (target_x - tab.navi.x_offset) * t as f64;
        let new_y = tab.navi.y_offset + (target_y - tab.navi.y_offset) * t as f64;
        tab.navi.x_offset = new_x;
        tab.navi.y_offset = new_y;
        follow_active = true;
        ui.ctx().request_repaint();
    } else {
        // Trip not found at this time — journey ended or entity despawned
        if repeat_time <= 0.0 {
            tab.navi.following = None;
        }
    }
}

let user_moved = tab.navi.handle_navigation(ui, &response);
```

Note: `handle_navigation` already returns `bool` indicating if there was user input.

- [ ] **Step 2: Exit follow on user input**

After the `handle_navigation` call, add:

```rust
// Exit follow mode on user input (soft follow)
if follow_active && user_moved {
    tab.navi.following = None;
}
// Also exit on Escape
if tab.navi.following.is_some() && ui.input(|i| i.key_pressed(Key::Escape)) {
    tab.navi.following = None;
}
```

Add `Key` to the existing egui imports at the top of the file.

- [ ] **Step 3: Verify it compiles**

Run: `cargo clippy --workspace`
Expected: No new errors or warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/paiagram-ui/src/tabs/graph.rs
git commit -m "feat(graph): implement follow viewport tracking logic"
```

---

## Chunk 3: Click Interaction & Visual Feedback

### Task 4: Add click-to-follow interaction

**Files:**
- Modify: `crates/paiagram-ui/src/tabs/graph.rs:344-365` (selection handling in `display()`)

- [ ] **Step 1: Set follow target on trip click**

In the selection match block at line 344, when a `TimetableEntries` selection is made, also set the follow target. Modify the match arm:

```rust
match (selected_item, selected_items) {
    (Some(SelectedItem::Stations(station)), SelectedItems::Stations(stations))
        if shift_pressed && stations.len() == 1 =>
    {
        // ...existing interval creation logic...
    }
    (Some(SelectedItem::TimetableEntries(entry)), items) => {
        // Set follow target to the trip
        tab.navi.following = Some(entry.parent);
        let ctrl_pressed = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        if ctrl_pressed {
            items.add_entry(SelectedItem::TimetableEntries(entry));
        } else {
            items.set_or_reset(SelectedItem::TimetableEntries(entry));
        }
    }
    (Some(item), items) => {
        let ctrl_pressed = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        if ctrl_pressed {
            items.add_entry(item);
        } else {
            items.set_or_reset(item);
        }
    }
    (None, _) => {}
}
```

- [ ] **Step 2: Auto-start playback when entering follow mode**

The `GlobalTimer` is mutated in `show_ui` (lib.rs) in the top panel, not in `display()`. Since `display()` receives `world: &mut World`, we can mutate it after the selection handling:

```rust
// Auto-start playback when entering follow mode
if tab.navi.following.is_some() {
    let timer = world.resource_mut::<GlobalTimer>();
    if !timer.animation_playing {
        timer.into_inner().animation_playing = true;
    }
}
```

Place this right after the selection match block.

- [ ] **Step 3: Verify it compiles**

Run: `cargo clippy --workspace`
Expected: No new errors or warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/paiagram-ui/src/tabs/graph.rs
git commit -m "feat(graph): add click-to-follow and auto-start playback"
```

---

### Task 5: Visual highlight for followed trip

**Files:**
- Modify: `crates/paiagram-ui/src/tabs/graph.rs:526-696` (`push_draw_items` function)

- [ ] **Step 1: Pass `following` entity to `push_draw_items`**

Add a new parameter to `push_draw_items` for the followed trip entity. Update the function signature:

```rust
fn push_draw_items(
    (
        In(is_dark),
        InRef(navi),
        InMut(buffer),
        InMut(painter),
        In(maybe_interact_pos),
        InRef(selected_stations),
        InRef(selected_trips),
        In(text_strength),
        In(following_trip),
    ): (
        In<bool>,
        InRef<GraphNavigation>,
        InMut<Vec<ShapeInstance>>,
        InMut<Painter>,
        In<Option<Pos2>>,
        InRef<[Entity]>,
        InRef<[Entity]>,
        In<f32>,
        In<Option<Entity>>,
    ),
    // ...rest unchanged
```

- [ ] **Step 2: Update the call site in `display()`**

In the `world.run_system_cached_with(push_draw_items, (...))` call (around line 266), add the following entity:

```rust
let selected_item = world
    .run_system_cached_with(
        push_draw_items,
        (
            ui.visuals().dark_mode,
            &tab.navi,
            &mut state.instances,
            &mut painter,
            interact_pos,
            &selected_stations,
            &selected_trips,
            ui.ctx()
                .animate_bool(ui.id().with("gugugaga"), tab.navi.zoom > 0.002),
            tab.navi.following,
        ),
    )
    .unwrap();
```

- [ ] **Step 3: Add visual highlight in the trip rendering loop**

In the trip rendering loop (around line 640-694), add highlighting for the followed trip. After the existing `selected_trips.contains` highlight (line 673-680), add:

```rust
if Some(sample.trip) == following_trip {
    // Draw a larger, brighter highlight for the followed trip
    painter.circle(
        pos,
        SELECTION_RADIUS + 4.0,
        Color32::GOLD.gamma_multiply(0.3),
        Stroke::new(2.0, Color32::GOLD),
    );
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo clippy --workspace`
Expected: No new errors or warnings.

- [ ] **Step 5: Manual test**

Run: `cargo run`
1. Open a Graph tab with trips loaded
2. Click play animation — verify trips move as arrows
3. Click a trip arrow — viewport should smoothly follow it
4. The followed trip should have a gold highlight ring
5. Pan with mouse — follow mode should exit
6. Click another trip — should switch follow target
7. Press Escape — should exit follow mode
8. Click a trip when animation is paused — animation should auto-start

- [ ] **Step 6: Commit**

```bash
git add crates/paiagram-ui/src/tabs/graph.rs crates/paiagram-core/src/trip.rs
git commit -m "feat(graph): add Trip follow mode with visual highlight"
```
