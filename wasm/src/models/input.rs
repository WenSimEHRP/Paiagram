use crate::types::*;
use anyhow::Result;
use serde::Deserialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use multimap::MultiMap;

/// hash string to ids
fn hash_id(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[derive(Deserialize)]
#[serde(try_from = "NetworkHelper")]
pub struct Network {
    pub stations: HashMap<StationID, Station>,
    pub trains: HashMap<TrainID, Train>,
    pub intervals: HashMap<IntervalID, Interval>,
}

#[derive(Deserialize)]
struct NetworkHelper {
    stations: HashMap<String, StationHelper>,
    trains: HashMap<String, TrainHelper>,
    intervals: Vec<((String, String), IntervalHelper)>,
}

impl TryFrom<NetworkHelper> for Network {
    type Error = anyhow::Error;
    fn try_from(helper: NetworkHelper) -> Result<Self, Self::Error> {
        let mut stations: HashMap<StationID, Station> =
            HashMap::with_capacity(helper.stations.len());
        let mut trains: HashMap<TrainID, Train> = HashMap::with_capacity(helper.trains.len());
        let mut intervals: HashMap<IntervalID, Interval> =
            HashMap::with_capacity(helper.intervals.len());
        for (name, helper) in helper.stations {
            let id = hash_id(&name);
            let station = Station {
                milestones: helper.milestones,
                tracks: helper.tracks.unwrap_or(1),
                name,
                intervals: HashSet::new(),
                trains: HashSet::new(),
            };
            stations.insert(id, station);
        }
        for ((from, to), interval) in helper.intervals {
            let from_id = hash_id(&from);
            let to_id = hash_id(&to);
            let id = (from_id, to_id);
            let new_interval = Interval {
                name: interval.name,
                length: interval.length,
            };
            match interval.bidirectional {
                Some(true) | None => {
                    if intervals.contains_key(&id.reverse()) {
                        return Err(anyhow::anyhow!(
                            "Interval from '{}' to '{}' already exists",
                            to,
                            from
                        ));
                    }
                    intervals.insert(id.reverse(), new_interval.clone());
                    if intervals.contains_key(&id) {
                        return Err(anyhow::anyhow!(
                            "Interval from '{}' to '{}' already exists",
                            from,
                            to
                        ));
                    }
                    intervals.insert(id, new_interval);
                }
                _ => {
                    if intervals.contains_key(&id) {
                        return Err(anyhow::anyhow!(
                            "Interval from '{}' to '{}' already exists",
                            from,
                            to
                        ));
                    }
                    intervals.insert(id, new_interval);
                }
            }
            if let Some(from_station) = stations.get_mut(&from_id) {
                from_station.intervals.insert(id);
            }
            if let Some(to_station) = stations.get_mut(&to_id) {
                to_station.intervals.insert(id);
            }
        }
        for (name, helper) in helper.trains {
            let id = hash_id(&name);
            let mut schedule = Vec::with_capacity(helper.schedule.len());
            let mut schedule_index: MultiMap<StationID, usize> = MultiMap::new();
            for (idx, entry) in helper.schedule.into_iter().enumerate() {
                let station_id = hash_id(&entry.station);
                schedule_index.insert(station_id, idx);
                if let Some(station) = stations.get_mut(&station_id) {
                    station.trains.insert(id);
                }
                schedule.push(ScheduleEntry {
                    arrival: entry.arrival,
                    departure: entry.departure,
                    station: station_id,
                    actions: entry.actions.unwrap_or_default(),
                });
            }
            trains.insert(id, Train { name, schedule, schedule_index });
        }
        Ok(Network {
            stations,
            trains,
            intervals,
        })
    }
}

pub struct Station {
    pub milestones: Option<HashMap<String, IntervalLength>>,
    pub tracks: u16,
    pub name: String,
    // those fields are completed afterwards
    pub intervals: HashSet<IntervalID>,
    pub trains: HashSet<TrainID>,
}

#[derive(Deserialize)]
struct StationHelper {
    milestones: Option<HashMap<String, IntervalLength>>,
    tracks: Option<u16>,
}

pub struct Train {
    pub name: String,
    pub schedule: Vec<ScheduleEntry>,
    pub schedule_index: MultiMap<StationID, usize>,
}

#[derive(Deserialize)]
struct TrainHelper {
    schedule: Vec<ScheduleEntryHelper>,
}

pub struct ScheduleEntry {
    pub arrival: Time,
    pub departure: Time,
    pub station: StationID,
    pub actions: HashSet<TrainAction>,
}

#[derive(Deserialize)]
struct ScheduleEntryHelper {
    arrival: Time,
    departure: Time,
    station: String,
    actions: Option<HashSet<TrainAction>>,
}

#[derive(Clone)]
pub struct Interval {
    pub name: Option<String>,
    pub length: IntervalLength,
}

#[derive(Deserialize)]
struct IntervalHelper {
    name: Option<String>,
    length: IntervalLength,
    bidirectional: Option<bool>,
}

#[derive(Deserialize)]
#[serde(try_from = "NetworkConfigHelper")]
pub struct NetworkConfig {
    pub stations_to_draw: Vec<StationID>,
    pub beg: Time,
    pub end: Time,
    pub unit_length: GraphLength,
    pub position_axis_scale_mode: ScaleMode,
    pub time_axis_scale_mode: ScaleMode,
}

#[derive(Deserialize)]
struct NetworkConfigHelper {
    stations_to_draw: Vec<String>,
    beg: Time,
    end: Time,
    unit_length: GraphLength,
    position_axis_scale_mode: ScaleMode,
    time_axis_scale_mode: ScaleMode,
}

impl TryFrom<NetworkConfigHelper> for NetworkConfig {
    type Error = anyhow::Error;
    fn try_from(helper: NetworkConfigHelper) -> Result<Self, Self::Error> {
        if helper.stations_to_draw.is_empty() {
            return Err(anyhow::anyhow!(
                "You must specify at least one station to draw"
            ));
        }
        let mut stations_to_draw = Vec::with_capacity(helper.stations_to_draw.len());
        let mut intervals: HashSet<IntervalID> =
            HashSet::with_capacity(helper.stations_to_draw.len() - 1);
        for i in 0..helper.stations_to_draw.len() {
            let to = hash_id(&helper.stations_to_draw[i]);
            stations_to_draw.push(to);
            // skip this for the first element
            if i == 0 {
                continue;
            }
            // check if the previous station is the same as the current one
            let from = stations_to_draw[i - 1];
            if from == to {
                return Err(anyhow::anyhow!(
                    "Consecutive stations cannot be the same: {}",
                    helper.stations_to_draw[i]
                ));
            }
            if !intervals.insert((from, to)) {
                return Err(anyhow::anyhow!(
                    "Duplicate interval from '{}' to '{}'",
                    helper.stations_to_draw[i - 1],
                    helper.stations_to_draw[i]
                ));
            }
            if !intervals.insert((to, from)) {
                return Err(anyhow::anyhow!(
                    "Duplicate interval from '{}' to '{}'",
                    helper.stations_to_draw[i],
                    helper.stations_to_draw[i - 1]
                ));
            }
        }
        if helper.beg.seconds() > helper.end.seconds() {
            return Err(anyhow::anyhow!(
                "The beginning time cannot be after the end time"
            ));
        }
        Ok(NetworkConfig {
            stations_to_draw,
            beg: helper.beg,
            end: helper.end,
            unit_length: helper.unit_length,
            position_axis_scale_mode: helper.position_axis_scale_mode,
            time_axis_scale_mode: helper.time_axis_scale_mode,
        })
    }
}
