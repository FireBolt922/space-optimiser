use std::path::PathBuf;

use space_optimiser_engine::Engine;

fn run(engine: &Engine) {
    let _ = engine.execute_plan(PathBuf::from("/Users/me/Documents"));
}

fn main() {}
