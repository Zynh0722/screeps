#![feature(hash_extract_if)]

use std::cell::RefCell;
use std::collections::HashMap;

use rand::rngs::SmallRng;
use rand::SeedableRng;
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
}

thread_local! {
    static CREEP_TARGETS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

mod timer;

// to use a reserved name as a function name, use `js_name`:
#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    // info!("loop starting! CPU: {}", game::cpu::get_used());
    let _timer = timer::TimerLog::start("Main Loop".to_owned());
}
