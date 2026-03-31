mod models;
mod phash;
mod sensor;

use std::time::Duration;

fn main() {
    // For now we run a simple demo loop that simulates events and shows how
    // the sensor layer processes them. You can later replace the simulated
    // source with real OS hooks and Tauri integration.
    let mut engine = sensor::SensorEngine::new();

    // Simulate some activity every second for demonstration.
    for _ in 0..10 {
        // In a real app, this would be called from OS event callbacks.
        engine.handle_event(sensor::SensorEvent::SimulatedActivity);

        std::thread::sleep(Duration::from_secs(1));
    }

    // At the end, dump the collected Phase 1 → Phase 2 payloads as JSON.
    let payloads = engine.drain_phase2_payloads();
    println!(
        "{}",
        serde_json::to_string_pretty(&payloads).expect("serialize payloads")
    );
}

