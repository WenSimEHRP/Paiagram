use crate::collision::*;
use crate::input::*;
use crate::types::*;
use crate::utils::intersection;
use anyhow::{Result, anyhow};
use multimap::MultiMap;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

enum LabelDirection {
    Up,
    Down,
    // Left,
    // Right,
}

enum LabelPosition {
    Beg(LabelDirection),
    End(LabelDirection),
}

#[derive(Serialize)]
struct OutputTrain {
    edges: Vec<OutputEdge>,
    name: String,
}

#[derive(Serialize)]
struct OutputEdge {
    edges: Vec<Node>,
    labels: Option<OutputLabel>,
}

#[derive(Serialize)]
struct OutputLabel {
    angles: (f64, f64),
}

#[derive(Serialize)]
pub struct Output {
    collision_manager: CollisionManager,
    trains: Vec<OutputTrain>,
    graph_intervals: Vec<GraphLength>,
    #[serde(skip)]
    station_draw_info: Vec<(StationID, GraphLength, LineCollisionManager)>,
    #[serde(skip)]
    station_indices: MultiMap<StationID, usize>,
    #[serde(skip)]
    config: NetworkConfig,
}

impl Output {
    pub fn new(config: NetworkConfig) -> Self {
        let collision_manager = CollisionManager::new(config.unit_length);

        Self {
            collision_manager,
            trains: Vec::new(),
            station_draw_info: Vec::with_capacity(config.stations_to_draw.len()),
            station_indices: MultiMap::with_capacity(config.stations_to_draw.len()),
            graph_intervals: Vec::with_capacity(config.stations_to_draw.len().saturating_sub(1)),
            config,
        }
    }

    pub fn populate(&mut self, network: Network) -> Result<()> {
        let train_ids_to_draw =
            self.make_station_draw_info(&network.stations, &network.intervals)?;

        let time_unit_length = self.config.unit_length * self.config.time_axis_scale;

        self.collision_manager.update_x_min(GraphLength::from(
            self.config
                .start_time
                .to_graph_length(time_unit_length)
                .value(),
        ));
        self.collision_manager.update_x_max(GraphLength::from(
            self.config
                .end_time
                .to_graph_length(time_unit_length)
                .value(),
        ));
        self.collision_manager.update_y_min(GraphLength::from(
            self.station_draw_info
                .first()
                .map_or(0.0, |(_, y, _)| y.value()),
        ));
        self.collision_manager.update_y_max(GraphLength::from(
            self.station_draw_info
                .last()
                .map_or(0.0, |(_, y, _)| y.value()),
        ));

        self.trains.reserve(train_ids_to_draw.len());
        for train_id in train_ids_to_draw {
            let output_train = self.make_train(network.trains.get(&train_id).unwrap())?;
            self.trains.push(output_train);
        }

        Ok(())
    }

    fn make_station_draw_info(
        &mut self,
        stations: &HashMap<StationID, Station>,
        intervals: &HashMap<IntervalID, Interval>,
    ) -> Result<HashSet<TrainID>> {
        if self.config.stations_to_draw.is_empty() {
            return Err(anyhow!("No stations to draw"));
        }

        // check if all stations to draw exist
        for &station_id in &self.config.stations_to_draw {
            if !stations.contains_key(&station_id) {
                return Err(anyhow!("Station {} not found", station_id));
            }
        }

        let train_ids: HashSet<TrainID> = self
            .config
            .stations_to_draw
            .iter()
            .filter_map(|id| stations.get(id))
            .flat_map(|station| &station.trains)
            .copied()
            .collect();

        let mut position: GraphLength = 0.0.into();

        let unit_length = self.config.unit_length * self.config.position_axis_scale;
        let label_start = GraphLength::from(0.0f64);

        // process the first station
        let first_station = self.config.stations_to_draw[0];
        self.station_draw_info
            .push((first_station, position, LineCollisionManager::new()));
        self.station_indices.insert(first_station, 0);
        // handle the first station label
        let (width, height) = stations.get(&first_station).unwrap().label_size;
        self.collision_manager.add_collision(vec![
            Node(label_start - width - 3.0.into(), position - height * 0.5),
            Node(label_start - 3.0.into(), position - height * 0.5),
            Node(label_start - 3.0.into(), position + height * 0.5),
            Node(label_start - width - 3.0.into(), position + height * 0.5),
        ])?;

        for (window_idx, window) in self.config.stations_to_draw.windows(2).enumerate() {
            let [start_station, end_station] = window else {
                continue;
            };
            if *start_station == *end_station {
                return Err(anyhow!("Consecutive stations cannot be the same"));
            }

            let interval_length = match (
                intervals
                    .get(&(*start_station, *end_station))
                    .map(|it| it.length),
                intervals
                    .get(&(*end_station, *start_station))
                    .map(|it| it.length),
            ) {
                (Some(len1), Some(len2)) => {
                    IntervalLength::new((len1.meters() + len2.meters()) / 2)
                        .to_graph_length(unit_length, self.config.position_axis_scale_mode)
                }
                (Some(len), None) | (None, Some(len)) => {
                    len.to_graph_length(unit_length, self.config.position_axis_scale_mode)
                }
                (None, None) => {
                    // return Err(anyhow!(
                    //     "No interval found between stations {} and {}",
                    //     start_station,
                    //     end_station
                    // ));
                    self.config.unit_length
                }
            };

            self.graph_intervals.push(interval_length);
            position += interval_length;
            self.station_draw_info
                .push((*end_station, position, LineCollisionManager::new()));
            self.station_indices.insert(*end_station, window_idx + 1);

            let (width, height) = stations.get(end_station).unwrap().label_size;
            // insert station label. nodes are in absolute coordinates
            self.collision_manager.add_collision(vec![
                Node(label_start - width - 3.0.into(), position - height * 0.5),
                Node(label_start - 3.0.into(), position - height * 0.5),
                Node(label_start - 3.0.into(), position + height * 0.5),
                Node(label_start - width - 3.0.into(), position + height * 0.5),
            ])?;
        }

        Ok(train_ids)
    }

    /// Make edges and place labels for each train
    fn make_train(&mut self, train: &Train) -> Result<OutputTrain> {
        let schedule = &train.schedule;
        // the GLOBAL edge group
        let mut output_edges: Vec<OutputEdge> = Vec::new();
        // the LOCAL edge group containing all WIP edges. The second element in the tuple
        // is the index to station_draw_info, which holds all station lines.
        let mut local_edges: Vec<(Vec<Node>, usize)> = Vec::new();
        let unit_length = self.config.unit_length * self.config.time_axis_scale;
        let map_end = (self.config.end_time - self.config.start_time).to_graph_length(unit_length);
        let map_start = GraphLength::from(0.0f64);

        let mut previous_indices: Option<&Vec<usize>> = self
            .station_indices
            .get_vec(&schedule.first().unwrap().station);
        for schedule_idx in 0..schedule.len() {
            // the current schedule entry
            let ce: &ScheduleEntry = schedule.get(schedule_idx).unwrap();
            // the last station entry does not have a next entry
            let ne: Option<&ScheduleEntry> = schedule.get(schedule_idx + 1);
            let Some(current_indices) = previous_indices else {
                if schedule_idx < schedule.len() - 1 {
                    let ne = &schedule[schedule_idx + 1];
                    previous_indices = self.station_indices.get_vec(&ne.station);
                }
                if local_edges.is_empty() {
                    continue;
                }
                output_edges.extend(std::mem::take(&mut local_edges).into_iter().map(
                    |(edge_nodes, _)| OutputEdge {
                        edges: edge_nodes,
                        labels: None,
                    },
                ));
                continue;
            };
            let edge_start = (ce.arrival - self.config.start_time).to_graph_length(unit_length);
            let edge_end = (ce.departure - self.config.start_time).to_graph_length(unit_length);
            let mut remaining_edges: Vec<(Vec<Node>, usize)> = Vec::new();
            let mut remaining_edge: Option<(Vec<Node>, usize)> = None;
            for &graph_index in current_indices {
                if let Some(it) = remaining_edge.take() {
                    remaining_edges.push(it);
                };
                let (_, current_base_height, ref mut current_collision_manager) =
                    self.station_draw_info[graph_index];
                if let Some(local_index) = local_edges
                    .iter()
                    .position(|(_, idx)| graph_index.abs_diff(*idx) == 1)
                {
                    // in this case the arrival time MUST be inside the map
                    let (mut matched_edge, _) = local_edges.swap_remove(local_index);
                    if ce.departure < self.config.start_time || ce.departure > self.config.end_time
                    {
                        // the edge extends to the right of the map
                        let height = current_base_height
                            + current_collision_manager.resolve_collisions(edge_start, map_end)?
                                as f64
                                * self.config.line_stack_space;
                        matched_edge
                            .extend_from_slice(&[Node(edge_start, height), Node(map_end, height)]);
                        output_edges.push(OutputEdge {
                            edges: matched_edge,
                            labels: None,
                        });
                    } else if ce.departure < ce.arrival {
                        // the edge extends to the right of the map, then turns back into the map
                        let height = current_base_height
                            + current_collision_manager.resolve_collisions(edge_start, map_end)?
                                as f64
                                * self.config.line_stack_space;
                        matched_edge
                            .extend_from_slice(&[Node(edge_start, height), Node(map_end, height)]);
                        output_edges.push(OutputEdge {
                            edges: matched_edge,
                            labels: None,
                        });
                        let height = current_base_height
                            + current_collision_manager.resolve_collisions(map_start, edge_end)?
                                as f64
                                * self.config.line_stack_space;
                        remaining_edge = Some((
                            vec![Node(map_start, height), Node(edge_end, height)],
                            graph_index,
                        ));
                    } else if ce.departure == ce.arrival {
                        // the edge is a point
                        matched_edge.push(Node(edge_start, current_base_height));
                        remaining_edge = Some((matched_edge, graph_index));
                    } else {
                        // the edge extends forwards but not beyond the map
                        let height = current_base_height
                            + current_collision_manager.resolve_collisions(edge_start, edge_end)?
                                as f64
                                * self.config.line_stack_space;
                        matched_edge
                            .extend_from_slice(&[Node(edge_start, height), Node(edge_end, height)]);
                        remaining_edge = Some((matched_edge, graph_index));
                    }
                } else {
                    if ce.arrival < self.config.start_time {
                        if ce.departure < self.config.start_time {
                            // do nothing
                        } else if ce.departure < self.config.end_time {
                            // draw normally
                            let height = current_base_height
                                + current_collision_manager
                                    .resolve_collisions(map_start, edge_end)?
                                    as f64
                                    * self.config.line_stack_space;
                            remaining_edge = Some((
                                vec![Node(map_start, height), Node(edge_end, height)],
                                graph_index,
                            ));
                        } else {
                            // the edge spams across the whole map
                            let height = current_base_height
                                + current_collision_manager
                                    .resolve_collisions(map_start, map_end)?
                                    as f64
                                    * self.config.line_stack_space;
                            output_edges.push(OutputEdge {
                                edges: vec![Node(map_start, height), Node(map_end, height)],
                                labels: None,
                            });
                        }
                    } else if ce.arrival < self.config.end_time {
                        let mut matched_edge = Vec::new();
                        if ce.departure < self.config.start_time
                            || ce.departure > self.config.end_time
                        {
                            // the edge extends to the right of the map
                            let height = current_base_height
                                + current_collision_manager
                                    .resolve_collisions(edge_start, map_end)?
                                    as f64
                                    * self.config.line_stack_space;
                            matched_edge.extend_from_slice(&[
                                Node(edge_start, height),
                                Node(map_end, height),
                            ]);
                            output_edges.push(OutputEdge {
                                edges: matched_edge,
                                labels: None,
                            });
                        } else if ce.departure < ce.arrival {
                            // the edge extends to the right of the map, then turns back into the map
                            let height = current_base_height
                                + current_collision_manager
                                    .resolve_collisions(edge_start, map_end)?
                                    as f64
                                    * self.config.line_stack_space;
                            matched_edge.extend_from_slice(&[
                                Node(edge_start, height),
                                Node(map_end, height),
                            ]);
                            output_edges.push(OutputEdge {
                                edges: matched_edge,
                                labels: None,
                            });
                            let height = current_base_height
                                + current_collision_manager
                                    .resolve_collisions(map_start, edge_end)?
                                    as f64
                                    * self.config.line_stack_space;
                            remaining_edge = Some((
                                vec![Node(map_start, height), Node(edge_end, height)],
                                graph_index,
                            ));
                        } else if ce.departure == ce.arrival {
                            // the edge is a point
                            matched_edge.push(Node(edge_start, current_base_height));
                            remaining_edge = Some((matched_edge, graph_index));
                        } else {
                            // the edge extends forwards but not beyond the map
                            let height = current_base_height
                                + current_collision_manager
                                    .resolve_collisions(edge_start, edge_end)?
                                    as f64
                                    * self.config.line_stack_space;
                            matched_edge.extend_from_slice(&[
                                Node(edge_start, height),
                                Node(edge_end, height),
                            ]);
                            remaining_edge = Some((matched_edge, graph_index));
                        }
                    } else {
                        if ce.departure < self.config.start_time {
                            // do nothing
                        } else if ce.departure < self.config.end_time {
                            // draw normally
                            let height = current_base_height
                                + current_collision_manager
                                    .resolve_collisions(map_start, edge_end)?
                                    as f64
                                    * self.config.line_stack_space;
                            remaining_edge = Some((
                                vec![Node(map_start, height), Node(edge_end, height)],
                                graph_index,
                            ));
                        } else {
                            // the edge spams across the whole map
                            let height = current_base_height
                                + current_collision_manager
                                    .resolve_collisions(map_start, map_end)?
                                    as f64
                                    * self.config.line_stack_space;
                            output_edges.push(OutputEdge {
                                edges: vec![Node(map_start, height), Node(map_end, height)],
                                labels: None,
                            });
                        }
                    }
                }

                // The same station cannot appear twice in a row, neither can they appear
                // with only one station in between. This means that for this specific entry,
                // there can only be one adjacent next station on the graph.
                // This limitation is what makes the following code work.

                let Some(ne) = ne else {
                    // there isn't a next station
                    continue;
                };

                // query the next station's position
                // there might be an index out of bounds error here, so use the safe method
                // TODO: track settings
                let Some(((_, next_base_height, _), _compare_result)) = (graph_index
                    .saturating_sub(1)
                    ..=graph_index.saturating_add(1))
                    .find_map(|idx| {
                        let base_line = self.station_draw_info.get(idx)?;
                        if base_line.0 == ne.station {
                            Some((base_line, graph_index.cmp(&idx)))
                        } else {
                            None
                        }
                    })
                else {
                    continue;
                };

                let next_edge_start =
                    (ne.arrival - self.config.start_time).to_graph_length(unit_length);

                if ce.departure < self.config.start_time {
                    // remaining edge MUST be none
                    if ne.arrival < self.config.start_time {
                        // do nothing
                    } else if ne.arrival < self.config.end_time {
                        let mut op_vec = Vec::with_capacity(2);
                        let inter_node = intersection(
                            Node(edge_end, current_base_height),
                            Node(next_edge_start, *next_base_height),
                            map_start,
                        )?;
                        op_vec.push(inter_node);
                        remaining_edge = Some((op_vec, graph_index));
                    } else {
                        let mut op_vec = Vec::with_capacity(2);
                        let start_inter_node = intersection(
                            Node(edge_end, current_base_height),
                            Node(next_edge_start, *next_base_height),
                            map_start,
                        )?;
                        let end_inter_node = intersection(
                            Node(edge_end, current_base_height),
                            Node(next_edge_start, *next_base_height),
                            map_end,
                        )?;
                        op_vec.extend_from_slice(&[start_inter_node, end_inter_node]);
                        output_edges.push(OutputEdge {
                            edges: op_vec,
                            labels: None,
                        });
                    }
                } else if ce.departure < self.config.end_time {
                    // in this case remaining_edge MUST be Some
                    let (mut matched_edge, _) = remaining_edge.take().unwrap();
                    if ne.arrival < self.config.start_time {
                        let inter_node = intersection(
                            Node(edge_end, current_base_height),
                            Node(
                                ne.arrival.tomorrow().to_graph_length(unit_length),
                                *next_base_height,
                            ),
                            map_end,
                        )?;
                        matched_edge.push(inter_node);
                        output_edges.push(OutputEdge {
                            edges: matched_edge,
                            labels: None,
                        });
                    } else if ne.arrival < ce.departure {
                        let end_inter_node = intersection(
                            Node(edge_end, current_base_height),
                            Node(
                                ne.arrival.tomorrow().to_graph_length(unit_length),
                                *next_base_height,
                            ),
                            map_end,
                        )?;
                        matched_edge.push(end_inter_node);
                        output_edges.push(OutputEdge {
                            edges: matched_edge,
                            labels: None,
                        });
                        let start_inter_node = intersection(
                            Node(
                                ce.departure.yesterday().to_graph_length(unit_length),
                                current_base_height,
                            ),
                            Node(next_edge_start, *next_base_height),
                            map_start,
                        )?;
                        remaining_edge = Some((vec![start_inter_node], graph_index));
                    } else if ne.arrival < self.config.end_time {
                        // don't process it for now
                        remaining_edge = Some((matched_edge, graph_index));
                    } else {
                        let end_inter_node = intersection(
                            Node(edge_end, current_base_height),
                            Node(next_edge_start, *next_base_height),
                            map_end,
                        )?;
                        matched_edge.push(end_inter_node);
                        output_edges.push(OutputEdge {
                            edges: matched_edge,
                            labels: None,
                        });
                    }
                } else {
                    if ne.arrival < self.config.start_time {
                        // do nothing
                    } else if ne.arrival < self.config.end_time {
                        let inter_node = intersection(
                            Node(
                                ce.departure.yesterday().to_graph_length(unit_length),
                                current_base_height,
                            ),
                            Node(next_edge_start, *next_base_height),
                            map_start,
                        )?;
                        remaining_edge = Some((vec![inter_node], graph_index));
                    } else {
                        // do nothing
                    }
                }
            }
            if let Some(remaining_edge) = remaining_edge {
                remaining_edges.push(remaining_edge);
            }
            if !local_edges.is_empty() {
                output_edges.extend(std::mem::take(&mut local_edges).into_iter().map(
                    |(edge_nodes, _)| OutputEdge {
                        edges: edge_nodes,
                        labels: None,
                    },
                ));
            }
            local_edges = remaining_edges;
            if let Some(ne) = ne {
                previous_indices = self.station_indices.get_vec(&ne.station)
            };
        }
        // handle the remaining local edges
        output_edges.extend(local_edges.into_iter().map(|(edge_nodes, _)| OutputEdge {
            edges: edge_nodes,
            labels: None,
        }));

        // Filter out edges with less than 2 nodes before processing labels
        output_edges.retain(|output_edge| output_edge.edges.len() >= 2);

        // iterate over all edges and add collision nodes
        let (label_width, label_height) = train.label_size;
        for output_edge in &mut output_edges {
            let angles =
                self.add_train_labels_to_edge(&mut output_edge.edges, label_width, label_height)?;
            output_edge.labels = Some(OutputLabel { angles })
        }

        Ok(OutputTrain {
            edges: output_edges,
            name: train.name.clone(),
        })
    }

    fn create_label_polygon(
        &self,
        anchor: Node,
        label_width: GraphLength,
        label_height: GraphLength,
        direction: &LabelPosition,
    ) -> (Vec<Node>, f64, f64) {
        match direction {
            LabelPosition::Beg(dir) => {
                let polygon = vec![
                    Node(anchor.0 - label_width, anchor.1 - label_height),
                    Node(anchor.0, anchor.1 - label_height),
                    anchor,
                    Node(anchor.0 - label_width, anchor.1),
                ];
                match dir {
                    // the up and downs are reversed for typst
                    LabelDirection::Up => (
                        rotate_polygon(polygon, anchor, -self.config.label_angle),
                        90.0f64.to_radians(),
                        -self.config.label_angle,
                    ),
                    _ => (
                        rotate_polygon(polygon, anchor, self.config.label_angle),
                        -90.0f64.to_radians(),
                        self.config.label_angle,
                    ),
                }
            }
            LabelPosition::End(dir) => {
                let polygon = vec![
                    Node(anchor.0, anchor.1 - label_height),
                    Node(anchor.0 + label_width, anchor.1 - label_height),
                    Node(anchor.0 + label_width, anchor.1),
                    anchor,
                ];
                match dir {
                    LabelDirection::Up => (
                        rotate_polygon(polygon, anchor, -self.config.label_angle),
                        -90.0f64.to_radians(),
                        -self.config.label_angle,
                    ),
                    _ => (
                        rotate_polygon(polygon, anchor, self.config.label_angle),
                        90.0f64.to_radians(),
                        self.config.label_angle,
                    ),
                }
            }
        }
    }

    fn add_train_labels_to_edge(
        &mut self,
        edge: &mut Vec<Node>,
        label_width: GraphLength,
        label_height: GraphLength,
    ) -> Result<(f64, f64)> {
        let edge_start = *edge.first().unwrap();
        let edge_end = *edge.last().unwrap();

        let start_label_direction = if edge.len() > 2 {
            // check the first three nodes to determine general direction
            let (first, second, third) = (edge[0], edge[1], edge[2]);
            // check if the general trend is upwards
            // typst logic is reversed, so the directions are reversed
            if (second.1 > first.1) || (third.1 > second.1) {
                LabelPosition::Beg(LabelDirection::Down)
            } else {
                LabelPosition::Beg(LabelDirection::Up)
            }
        } else {
            let (first, last) = (edge[0], edge[1]);
            if first.1 < last.1 {
                LabelPosition::Beg(LabelDirection::Down)
            } else {
                LabelPosition::Beg(LabelDirection::Up)
            }
        };

        // Add label at the beginning of the edge
        let start_label_angle = self.add_label_to_edge(
            edge,
            edge_start,
            label_width,
            label_height,
            &start_label_direction,
        )?;

        // Add label at the end of the edge (only if edge has more than one node)

        // Determine direction for end label (might be different from beginning)
        let end_label_direction = if edge.len() > 2 {
            // check the last three nodes to determine general direction
            let (last, second_last, third_last) = (
                edge[edge.len() - 1],
                edge[edge.len() - 2],
                edge[edge.len() - 3],
            );
            // check if the general trend is upwards
            if (last.1 < second_last.1) || (second_last.1 < third_last.1) {
                LabelPosition::End(LabelDirection::Up)
            } else {
                LabelPosition::End(LabelDirection::Down)
            }
        } else {
            let (first, last) = (edge[0], edge[1]);
            if last.1 < first.1 {
                LabelPosition::End(LabelDirection::Up)
            } else {
                LabelPosition::End(LabelDirection::Down)
            }
        };

        // Insert at the end
        let end_label_angle = self.add_label_to_edge(
            edge,
            edge_end,
            label_width,
            label_height,
            &end_label_direction,
        )?;

        Ok((start_label_angle, end_label_angle))
    }

    fn add_label_to_edge(
        &mut self,
        edge: &mut Vec<Node>,
        anchor_point: Node,
        label_width: GraphLength,
        label_height: GraphLength,
        label_direction: &LabelPosition,
    ) -> Result<f64> {
        let (polygon, movement_angle, label_angle) =
            self.create_label_polygon(anchor_point, label_width, label_height, label_direction);

        let (resolved_polygon, _) = self
            .collision_manager
            .resolve_collisions(polygon, movement_angle)?;

        // Insert label nodes based on direction
        match label_direction {
            LabelPosition::Beg(_) => {
                edge.insert(0, resolved_polygon[2]);
                edge.insert(0, resolved_polygon[3]);
            }
            LabelPosition::End(_) => {
                edge.push(resolved_polygon[3]);
                edge.push(resolved_polygon[2]);
            }
        }

        Ok(label_angle)
    }
}
