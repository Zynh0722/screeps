#![feature(hash_extract_if, inline_const, const_trait_impl, const_for)]

use std::cell::RefCell;
use std::collections::{hash_map::Entry, HashMap};

use itertools::Itertools;
use log::*;
use rand::rngs::SmallRng;
pub(crate) use rand::{Rng, SeedableRng};
use screeps::{
    constants::{ErrorCode, Part, ResourceType},
    enums::StructureObject,
    find, game,
    local::ObjectId,
    objects::{Creep, Source, StructureController},
    prelude::*,
};
use screeps::{
    ConstructionSite, PolyStyle, RoomObject, Structure, StructureExtension, StructureSpawn,
    StructureTower, Terrain,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

mod logging;

// add wasm_bindgen to any function you would like to expose for call from js
#[wasm_bindgen]
pub fn setup() {
    logging::setup_logging(logging::Info);
}

// this is one way to persist data between ticks within Rust's memory, as opposed to
// keeping state in memory on game objects - but will be lost on global resets!
thread_local! {
    static RNG: RefCell<SmallRng> = RefCell::new(SmallRng::seed_from_u64(200));

    static CREEP_TARGETS: RefCell<HashMap<String, CreepTarget>> = RefCell::new(HashMap::new());
}

trait SumParts {
    fn sum_parts(&self) -> u32;
}

impl SumParts for [Part] {
    fn sum_parts(&self) -> u32 {
        self.iter().map(|p| p.cost()).sum()
    }
}

// this enum will represent a creep's lock on a specific target object, storing a js reference
// to the object id so that we can grab a fresh reference to the object each successive tick,
// since screeps game objects become 'stale' and shouldn't be used beyond the tick they were fetched
#[non_exhaustive]
#[derive(Clone, Debug, Serialize)]
enum CreepTarget {
    Upgrade(ObjectId<StructureController>),
    Harvest(ObjectId<Source>),
    Construct(ObjectId<ConstructionSite>),
    Store(StoreTarget),
    Repair(ObjectId<Structure>),
}

#[derive(Clone, Debug, Serialize)]
enum StoreTarget {
    Extension(ObjectId<StructureExtension>),
    Spawn(ObjectId<StructureSpawn>),
    Tower(ObjectId<StructureTower>),
}

impl StoreTarget {
    fn resolve(&self) -> Option<ResolvedStoreTarget> {
        match self {
            StoreTarget::Extension(id) => match id.resolve() {
                Some(structure) => Some(ResolvedStoreTarget::Extension(structure)),
                None => None,
            },
            StoreTarget::Spawn(id) => match id.resolve() {
                Some(structure) => Some(ResolvedStoreTarget::Spawn(structure)),
                None => None,
            },
            StoreTarget::Tower(id) => match id.resolve() {
                Some(structure) => Some(ResolvedStoreTarget::Tower(structure)),
                None => None,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
enum ResolvedStoreTarget {
    #[serde(skip)]
    Extension(StructureExtension),
    #[serde(skip)]
    Spawn(StructureSpawn),
    #[serde(skip)]
    Tower(StructureTower),
}

impl HasStore for ResolvedStoreTarget {
    fn store(&self) -> screeps::Store {
        use ResolvedStoreTarget::*;

        match self {
            Extension(structure) => structure.store(),
            Spawn(structure) => structure.store(),
            Tower(structure) => structure.store(),
        }
    }
}

impl AsRef<RoomObject> for ResolvedStoreTarget {
    fn as_ref<'a>(&'a self) -> &'a RoomObject {
        use ResolvedStoreTarget::*;

        match self {
            Extension(structure) => structure.as_ref(),
            Spawn(structure) => structure.as_ref(),
            Tower(structure) => structure.as_ref(),
        }
    }
}

impl Transferable for ResolvedStoreTarget {}

#[derive(Deserialize, Debug)]
struct Memory {
    creeps: HashMap<String, serde_json::Value>,
}

trait DefaultMove {
    fn default_move_to<T>(&self, target: &T) -> Result<(), ErrorCode>
    where
        T: AsRef<RoomObject>;
}

impl DefaultMove for Creep {
    fn default_move_to<T>(&self, target: &T) -> Result<(), ErrorCode>
    where
        T: AsRef<RoomObject>,
    {
        self.move_to_with_options(
            target,
            Some(
                screeps::MoveToOptions::new()
                    .reuse_path(5)
                    .visualize_path_style(
                        PolyStyle::default()
                            .fill("black")
                            .stroke_width(0.15)
                            .opacity(0.1)
                            .line_style(screeps::LineDrawStyle::Dashed),
                    ),
            ),
        )
    }
}

// to use a reserved name as a function name, use `js_name`:
#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    // info!("loop starting! CPU: {}", game::cpu::get_used());
    let starting_time = game::cpu::get_used();
    let current_tick = game::time();

    if current_tick % 10 == 0 {
        CREEP_TARGETS.with_borrow(|ct_refcell| {
            info!("CREEP_TARGETS: {:#?}", ct_refcell);
        });
    }

    if current_tick % 60 == 0 {
        use js_sys::Reflect;
        let alive_creeps: Vec<String> = game::creeps().keys().collect();

        let raw_mem = screeps::memory::ROOT.clone();

        // info!("{raw_mem:#?}");

        let memory: Result<Memory, _> =
            serde_wasm_bindgen::from_value(JsValue::from(raw_mem.clone()));

        if let Ok(mut memory) = memory {
            let starting = memory.creeps.keys().count();
            let removed = memory
                .creeps
                .extract_if(|k, _v| !alive_creeps.contains(k))
                .count();

            Reflect::set(
                &screeps::memory::ROOT,
                &"creeps".into(),
                &serde_wasm_bindgen::to_value(&memory.creeps).unwrap(),
            )
            .unwrap();

            info!("\t| removed: {removed}\n\t| starting: {starting}");
        } else {
            warn!("Bad memory");
        }
    }

    for structure in game::structures().values() {
        if let StructureObject::StructureTower(tower) = structure {
            if let Some(target) = tower
                .pos()
                .find_closest_by_range(screeps::find::HOSTILE_CREEPS)
            {
                tower.attack(&target).unwrap_or_else(|e| {
                    warn!("unable to attack target: {:?}", e);
                });
            }
        }
    }

    // mutably borrow the creep_targets refcell, which is holding our creep target locks
    // in the wasm heap
    CREEP_TARGETS.with_borrow_mut(|mut creep_targets| {
        debug!("running creeps");
        for creep in game::creeps().values() {
            run_creep(&creep, &mut creep_targets);
        }
    });

    debug!("running spawns");
    let mut additional = 0;
    for spawn in game::spawns().values() {
        debug!("running spawn {}", String::from(spawn.name()));

        // TODO: improve this. Builder pattern maybe?
        const THRESHOLDS: &[(usize, u32, &[Part])] = &[
            (
                6,
                300,
                &[Part::Move, Part::Move, Part::Carry, Part::Carry, Part::Work],
            ),
            (
                20,
                550,
                &[
                    Part::Move,
                    Part::Move,
                    Part::Move,
                    Part::Carry,
                    Part::Carry,
                    Part::Work,
                    Part::Work,
                    Part::Work,
                ],
            ),
        ];

        if let Some(room) = spawn.room() {
            let current_creeps = game::creeps().keys().count();
            let energy_available = &room.energy_available();
            let body_types = game::creeps()
                .values()
                .map(|c| c.body())
                .map(|b| b.into_iter().map(|p| p.part()))
                .map(|b| {
                    b.map(|p| match p {
                        Part::Move => "M",
                        Part::Work => "W",
                        Part::Carry => "C",
                        Part::Attack => "A",
                        Part::RangedAttack => "RA",
                        Part::Tough => "T",
                        Part::Heal => "H",
                        Part::Claim => "C",
                        _ => "?",
                    })
                    .join("")
                })
                .fold(HashMap::new(), |mut acc, key| {
                    match acc.entry(key) {
                        Entry::Occupied(mut e) => {
                            let value = e.get();
                            e.insert(value + 1);
                        }
                        Entry::Vacant(e) => {
                            e.insert(0);
                        }
                    };
                    acc
                });

            let total_bodies: u32 = body_types.values().sum();
            let bars: HashMap<String, f64> = body_types
                .into_iter()
                .map(|(b, q)| (b, q as f64 / total_bodies as f64))
                .collect();

            for (name, ratio) in bars {
                let hashes = (10.0 * ratio).round() as usize;
                let mut bar = String::new();
                bar.push_str(&"#".repeat(hashes));
                bar.push_str(&" ".repeat(10 - hashes));
                info!("{: >10}:[{}]", name, bar)
            }

            info!("Current Creeps: {current_creeps} -- Energy Available: {energy_available}");

            if let Some(body) = THRESHOLDS
                .iter()
                .skip_while(|(threshold, _, _)| &current_creeps > threshold)
                .next()
                .filter(|(_, cost, _)| cost <= energy_available)
                .map(|(_, _, body)| body)
            {
                // create a unique name, spawn.
                let name_base = game::time();
                let name = format!("{}-{}", name_base, additional);
                // TODO: handle pathfinding and caching manually
                // note that this bot has a fatal flaw; spawning a creep
                // creates Memory.creeps[creep_name] which will build up forever;
                // these memory entries should be prevented (todo doc link on how) or cleaned up
                //
                // NOTE: to library author, this code isn't what adds entries to
                // Memory.creeps[creep_name], it is actually the use of Creep.moveTo in the
                // run_creep function
                match spawn.spawn_creep(&body, &name) {
                    Ok(()) => additional += 1,
                    Err(e) => warn!("couldn't spawn: {:?}", e),
                }
            }
        }
    }

    info!(
        "done!\nloading_cpu: {:.2}\n engine_cpu: {:.2}",
        starting_time,
        game::cpu::get_used() - starting_time
    )
}

fn run_creep(creep: &Creep, creep_targets: &mut HashMap<String, CreepTarget>) {
    if creep.spawning() {
        return;
    }
    let name = creep.name();
    debug!("running creep {}", name);

    let target = creep_targets.entry(name);
    match target {
        Entry::Occupied(entry) => {
            let creep_target = entry.get();
            match creep_target {
                CreepTarget::Upgrade(controller_id)
                    if creep.store().get_used_capacity(Some(ResourceType::Energy)) > 0 =>
                {
                    if let Some(controller) = controller_id.resolve() {
                        if creep.pos().in_range_to(controller.pos(), 3) {
                            creep.upgrade_controller(&controller).unwrap_or_else(|e| {
                                warn!("couldn't upgrade: {:?}", e);
                                entry.remove();
                            });
                        } else {
                            let _ = creep.default_move_to(&controller);
                        }
                    } else {
                        entry.remove();
                    }
                }
                CreepTarget::Harvest(source_id)
                    if creep.store().get_free_capacity(Some(ResourceType::Energy)) > 0 =>
                {
                    if let Some(source) = source_id.resolve() {
                        if creep.pos().is_near_to(source.pos()) {
                            creep.harvest(&source).unwrap_or_else(|e| {
                                warn!("couldn't harvest: {:?}", e);
                                entry.remove();
                            });
                        } else {
                            let _ = creep.default_move_to(&source);
                        }
                    } else {
                        entry.remove();
                    }
                }
                CreepTarget::Construct(source_id) => {
                    if let Some(source) = source_id.resolve() {
                        if creep.pos().in_range_to(source.pos(), 3) {
                            creep.build(&source).unwrap_or_else(|e| {
                                warn!("couldn't build: {:?}", e);
                                entry.remove();
                            });
                        } else {
                            let _ = creep.default_move_to(&source);
                        }
                    } else {
                        entry.remove();
                    }
                }
                CreepTarget::Store(source) => {
                    if let Some(source) = source.resolve() {
                        if creep.pos().is_near_to(source.pos()) {
                            creep
                                .transfer(&source, ResourceType::Energy, None)
                                .unwrap_or_else(|e| {
                                    warn!("couldn't transfer: {:?}", e);
                                    entry.remove();
                                })
                        } else {
                            let _ = creep.default_move_to(&source);
                        }
                    } else {
                        entry.remove();
                    }
                }
                CreepTarget::Repair(source) => {
                    if let Some(structure) = source.resolve() {
                        if creep.pos().in_range_to(structure.pos(), 3) {
                            creep.repair(&structure).unwrap_or_else(|e| {
                                warn!("couldn't repair: {:?}", e);
                            });
                            entry.remove();
                        } else {
                            let _ = creep.default_move_to(&structure);
                        }
                    }
                }
                _ => {
                    entry.remove();
                }
            };
        }
        Entry::Vacant(entry) => {
            // no target, let's find one depending on if we have energy
            let room = creep.room().expect("couldn't resolve creep room");
            'temp: {
                if creep.store().get_used_capacity(Some(ResourceType::Energy)) > 0 {
                    let all_structures = room.find(find::STRUCTURES, None);

                    // if controller needs a timer reset, fill it
                    for structure in all_structures.iter() {
                        if let StructureObject::StructureController(controller) = structure {
                            let time_to_downgrade = match controller.level() {
                                1 => 20_000,
                                2 => 10_000,
                                3 => 20_000,
                                4 => 40_000,
                                5 => 80_000,
                                6 => 120_000,
                                7 => 150_000,
                                8 => 200_000,
                                _ => 20_000,
                            };
                            if controller.ticks_to_downgrade() < time_to_downgrade - 5000 {
                                entry.insert(CreepTarget::Upgrade(controller.id()));
                                break 'temp;
                            }
                        }
                    }

                    // fill spawners
                    for structure in all_structures.iter() {
                        if let StructureObject::StructureSpawn(spawn) = structure {
                            if spawn.store().get_free_capacity(Some(ResourceType::Energy)) > 0 {
                                entry.insert(CreepTarget::Store(StoreTarget::Spawn(spawn.id())));
                                break 'temp;
                            }
                        }
                    }

                    // fill extensions
                    for structure in all_structures.iter() {
                        if let StructureObject::StructureExtension(extension) = structure {
                            if extension
                                .store()
                                .get_free_capacity(Some(ResourceType::Energy))
                                > 0
                            {
                                entry.insert(CreepTarget::Store(StoreTarget::Extension(
                                    extension.id(),
                                )));
                                break 'temp;
                            }
                        }
                    }

                    for structure in all_structures.iter() {
                        if let StructureObject::StructureTower(tower) = structure {
                            if tower.store().get_free_capacity(Some(ResourceType::Energy)) > 0 {
                                entry.insert(CreepTarget::Store(StoreTarget::Tower(tower.id())));
                                break 'temp;
                            }
                        }
                    }

                    for structure in all_structures.iter() {
                        if let StructureObject::StructureRoad(road) = structure {
                            info!("checking for terrain");
                            if let Ok(Some(terrain)) = road
                                .pos()
                                .look_for(screeps::look::TERRAIN)
                                .map(|l| l.into_iter().take(1).last())
                            {
                                let threshold = match terrain {
                                    Terrain::Plain => 5_000,
                                    Terrain::Swamp => 25_000,
                                    Terrain::Wall => 750_000,
                                };
                                let threshold = threshold * 8 / 10;
                                info!("threshold: {threshold}");

                                if road.hits() < threshold {
                                    let structure: &Structure = road.as_ref();
                                    entry.insert(CreepTarget::Repair(structure.id()));
                                    break 'temp;
                                }
                            }
                        }
                    }

                    // build things
                    for site in room.find(find::CONSTRUCTION_SITES, None).iter() {
                        if let Some(id) = site.try_id() {
                            entry.insert(CreepTarget::Construct(id));
                            break 'temp;
                        }
                    }

                    // default case, upgrade controller
                    for structure in all_structures.iter() {
                        if let StructureObject::StructureController(controller) = structure {
                            entry.insert(CreepTarget::Upgrade(controller.id()));
                            break 'temp;
                        }
                    }
                } else {
                    let sources = room.find(find::SOURCES_ACTIVE, None).clone();

                    let random_in_range: usize = RNG.with_borrow_mut({
                        let max = sources.len();
                        move |rng| {
                            let mut gen = move || rng.gen_range(0..max);
                            [gen(), gen()].into_iter().max().unwrap()
                        }
                    });
                    info!("random value: {random_in_range}");

                    let random_source = sources.get(random_in_range);

                    if let Some(source) = random_source {
                        entry.insert(CreepTarget::Harvest(source.id()));
                    }
                }
            }
        }
    }
}
