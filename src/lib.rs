#![feature(hash_extract_if)]

use std::cell::RefCell;
use std::collections::{hash_map::Entry, HashMap};

use log::*;
use rand::rngs::SmallRng;
use rand::{thread_rng, Rng, SeedableRng};
use screeps::{
    constants::{ErrorCode, Part, ResourceType},
    enums::StructureObject,
    find, game,
    local::ObjectId,
    objects::{Creep, Source, StructureController},
    prelude::*,
};
use screeps::{
    ConstructionSite, RoomObject, Structure, StructureExtension, StructureSpawn, Visual,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use web_sys::console::warn;

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
}

thread_local! {
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
        }
    }
}

#[derive(Clone, Debug, Serialize)]
enum ResolvedStoreTarget {
    #[serde(skip)]
    Extension(StructureExtension),
    #[serde(skip)]
    Spawn(StructureSpawn),
}

impl HasStore for ResolvedStoreTarget {
    fn store(&self) -> screeps::Store {
        use ResolvedStoreTarget::*;

        match self {
            Extension(structure) => structure.store(),
            Spawn(structure) => structure.store(),
        }
    }
}

impl AsRef<RoomObject> for ResolvedStoreTarget {
    fn as_ref<'a>(&'a self) -> &'a RoomObject {
        use ResolvedStoreTarget::*;

        match self {
            Extension(structure) => structure.as_ref(),
            Spawn(structure) => structure.as_ref(),
        }
    }
}

impl Transferable for ResolvedStoreTarget {}

#[derive(Deserialize, Debug)]
struct Memory {
    creeps: HashMap<String, serde_json::Value>,
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

    if current_tick % 60 == 0 {}

    // drawing creeo targets
    // CREEP_TARGETS.with_borrow(|creep_targets| {});

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

        if game::creeps().keys().count() < 8 {
            let body = [Part::Move, Part::Move, Part::Carry, Part::Carry, Part::Work];
            if spawn.room().unwrap().energy_available() >= body[..].sum_parts() {
                // create a unique name, spawn.
                let name_base = game::time();
                let name = format!("{}-{}", name_base, additional);
                // note that this bot has a fatal flaw; spawning a creep
                // creates Memory.creeps[creep_name] which will build up forever;
                // these memory entries should be prevented (todo doc link on how) or cleaned up
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
                        creep
                            .upgrade_controller(&controller)
                            .unwrap_or_else(|e| match e {
                                ErrorCode::NotInRange => {
                                    let _ = creep.move_to_with_options(
                                        &controller,
                                        Some(screeps::MoveToOptions::new().reuse_path(10)),
                                    );
                                }
                                _ => {
                                    warn!("couldn't upgrade: {:?}", e);
                                    entry.remove();
                                }
                            });
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
                            let _ = creep.move_to_with_options(
                                &source,
                                Some(screeps::MoveToOptions::new().reuse_path(5)),
                            );
                        }
                    } else {
                        entry.remove();
                    }
                }
                CreepTarget::Construct(source_id) => {
                    if let Some(source) = source_id.resolve() {
                        if creep.pos().is_near_to(source.pos()) {
                            creep.build(&source).unwrap_or_else(|e| {
                                warn!("couldn't build: {:?}", e);
                                entry.remove();
                            });
                        } else {
                            let _ = creep.move_to_with_options(
                                &source,
                                Some(screeps::MoveToOptions::new().reuse_path(5)),
                            );
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
                            let _ = creep.move_to_with_options(
                                &source,
                                Some(screeps::MoveToOptions::new().reuse_path(5)),
                            );
                        }
                    } else {
                        entry.remove();
                    }
                }
                CreepTarget::Repair(source) => {
                    if let Some(structure) = source.resolve() {
                        if creep.pos().is_near_to(structure.pos()) {
                            creep.repair(&structure).unwrap_or_else(|e| {
                                warn!("couldn't repair: {:?}", e);
                                entry.remove();
                            })
                        } else {
                            let _ = creep.move_to_with_options(
                                &structure,
                                Some(screeps::MoveToOptions::new().reuse_path(5)),
                            );
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
                    // if controller needs a timer reset, fill it
                    for structure in room.find(find::STRUCTURES, None).iter() {
                        if let StructureObject::StructureController(controller) = structure {
                            if controller.ticks_to_downgrade() < 7500 {
                                entry.insert(CreepTarget::Upgrade(controller.id()));
                                break 'temp;
                            }
                        }
                    }

                    // fill extensions
                    for structure in room.find(find::STRUCTURES, None).iter() {
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

                    // fill spawners
                    for structure in room.find(find::STRUCTURES, None).iter() {
                        if let StructureObject::StructureSpawn(spawn) = structure {
                            if spawn.store().get_free_capacity(Some(ResourceType::Energy)) > 0 {
                                entry.insert(CreepTarget::Store(StoreTarget::Spawn(spawn.id())));
                                break 'temp;
                            }
                        }
                    }

                    // build things
                    for site in room.find(find::CONSTRUCTION_SITES, None) {
                        if let Some(id) = site.try_id() {
                            entry.insert(CreepTarget::Construct(id));
                            break 'temp;
                        }
                    }

                    for structure in room.find(find::STRUCTURES, None).iter() {
                        if let StructureObject::StructureRoad(road) = structure {
                            if road.hits() < 4000 {
                                let structure: &Structure = road.as_ref();
                                entry.insert(CreepTarget::Repair(structure.id()));
                                break 'temp;
                            }
                        }
                    }

                    // default case, upgrade controller
                    for structure in room.find(find::STRUCTURES, None).iter() {
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
                            let rolls = [gen(), gen()];
                            rolls.into_iter().max().unwrap() // Bias the second node
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
