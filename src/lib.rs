use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::collections::HashMap;

use bevy_ecs::component::Component;
use bevy_ecs::world::World;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use screeps::ObjectId;
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

    static CREEP_TARGETS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());

    static WORLD: RefCell<World> = RefCell::new(World::default());
}

mod timer;

#[derive(Component)]
struct Id<T>(ObjectId<T>);

#[derive(Component)]
struct Task<T>(ObjectId<T>);

fn hello_world_system() {
    println!("Hellow world");
}

// to use a reserved name as a function name, use `js_name`:
#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    // info!("loop starting! CPU: {}", game::cpu::get_used());
    let _timer = timer::TimerLog::start("Main Loop".to_owned());

    let mut schedule = bevy_ecs::schedule::Schedule::default();

    schedule.add_systems(hello_world_system);

    WORLD.with_borrow_mut(|mut world| schedule.run(&mut world));
}
