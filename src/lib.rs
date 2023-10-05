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
    static CREEP_TARGETS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

// to use a reserved name as a function name, use `js_name`:
#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    // info!("loop starting! CPU: {}", game::cpu::get_used());
    let starting_cpu = game::cpu::get_used();
    let current_tick = game::time();

    info!(
        "done!\nloading_cpu: {:.2}\n engine_cpu: {:.2}",
        starting_cpu,
        game::cpu::get_used() - starting_cpu
    )
}
