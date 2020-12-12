//
// Copyright (c) Dell Inc., or its subsidiaries. All Rights Reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//

use pravega_wire_protocol::{commands::{EventCommand, SegmentReadCommand}, wire_commands::Decode};
use pravega_wire_protocol::wire_commands::{Encode, Requests, Replies};
use std::time::Instant;
use tracing::info;

fn benchmark_write_fields() {
    info!("benchmark_write_fields: BEGIN");
    let size: usize = 1*1024*1024;
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

    loop {
        // Code to benchmark
        let serialized_event = event.write_fields().expect("Encoding event");
        let segment_read_serialized = Replies::SegmentRead(SegmentReadCommand {
            segment: "segment".to_string(),
            offset: 0,
            at_tail: false,
            end_of_segment: false,
            data: serialized_event,
            request_id: 0,
        })
        .write_fields()
        .expect("error while encoding segment read ");

        // Benchmark framework
        num_data_bytes += size as u64;
        num_serialized_bytes += segment_read_serialized.len() as u64;
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
                info!("benchmark_write_fields: warmed up");
            }
        }
    }
    let dt = t0.elapsed().as_secs_f64();
    let megabytes_per_sec = (num_data_bytes as f64) * 1e-6 / dt;
    info!("benchmark_write_fields: END; dt={}, num_events={}, num_data_bytes={}, num_serialized_bytes={}, megabytes_per_sec={}",
        dt, num_events, num_data_bytes, num_serialized_bytes, megabytes_per_sec);
}

fn benchmark_read_from() {
    info!("benchmark_read_from: BEGIN");
    let size: usize = 1*1024*1024;
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
    let serialized_event = event.write_fields().expect("Encoding event");
    let segment_read_serialized = Replies::SegmentRead(SegmentReadCommand {
        segment: "segment".to_string(),
        offset: 0,
        at_tail: false,
        end_of_segment: false,
        data: serialized_event,
        request_id: 0,
    })
    .write_fields()
    .expect("error while encoding segment read ");

    loop {
        // Code to benchmark
        let reply: Replies = Replies::read_from(&segment_read_serialized).expect("decode wirecommand");
        match reply {
            Replies::SegmentRead(segment_read) => {
                let event_read = Requests::read_from(&segment_read.data[..]);
            },
            _ => panic!("bad reply")
        }

        // Benchmark framework
        num_data_bytes += size as u64;
        num_serialized_bytes += segment_read_serialized.len() as u64;
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
                info!("benchmark_read_from: warmed up");
            }
        }
    }
    let dt = t0.elapsed().as_secs_f64();
    let megabytes_per_sec = (num_data_bytes as f64) * 1e-6 / dt;
    info!("benchmark_read_from: END; dt={}, num_events={}, num_data_bytes={}, num_serialized_bytes={}, megabytes_per_sec={}",
        dt, num_events, num_data_bytes, num_serialized_bytes, megabytes_per_sec);
}

// Benchmark for wire protocol serializers and deserializers.
// Run with: cargo bench --bench benchmark_wireprotocol
fn main() {
    let _ = tracing_subscriber::fmt::try_init();
    benchmark_write_fields();
    benchmark_read_from();
}
