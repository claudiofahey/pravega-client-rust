//
// Copyright (c) Dell Inc., or its subsidiaries. All Rights Reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//

use pravega_wire_protocol::commands::{EventCommand};
use pravega_wire_protocol::wire_commands::{Encode, Requests};
use std::time::Instant;
use tracing::info;

// Benchmark for Requests::Event.write_fields().
// Run with: cargo bench --bench benchmark_wireprotocol
fn main() {
    let _ = tracing_subscriber::fmt::try_init();
    info!("benchmark_wireprotocol.main: BEGIN");
    let size: usize = 8*1024*1024;
    let mut t0 = Instant::now();
    let mut num_events: u64 = 0;
    let mut num_data_bytes: u64 = 0;
    let mut num_serialized_bytes: u64 = 0;
    let mut warm = false;

    // Prepare data
    let data_chunk = vec![0xAAu8; size];
    let event = Requests::Event(EventCommand {
        data: data_chunk,
    });
    let serialized_event_size = event.write_fields().expect("Encoding event").len();
    info!("benchmark_wireprotocol.main: size={}, serialized_event_size={}", size, serialized_event_size);

    loop {
        // Code to benchmark
        let event_data = event.write_fields().expect("Encoding event");

        // Benchmark framework
        num_data_bytes += size as u64;
        num_serialized_bytes += event_data.len() as u64;
        num_events += 1;
        if warm {
            if t0.elapsed().as_secs_f64() > 4.0 {
                break;
            }
        } else {
            if t0.elapsed().as_secs_f64() > 2.0 {
                t0 = Instant::now();
                num_data_bytes = 0;
                num_serialized_bytes = 0;
                num_events = 0;
                warm = true;
                info!("benchmark_wireprotocol.main: warmed up");
            }
        }
    }
    let dt = t0.elapsed().as_secs_f64();
    let megabytes_per_sec = (num_data_bytes as f64) * 1e-6 / dt;
    info!("benchmark_wireprotocol.main: END; dt={}, num_events={}, num_data_bytes={}, num_serialized_bytes={}, megabytes_per_sec={}",
        dt, num_events, num_data_bytes, num_serialized_bytes, megabytes_per_sec);
}
