//
// Copyright (c) Dell Inc., or its subsidiaries. All Rights Reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//

use pravega_wire_protocol::commands::{EventCommand, SegmentReadCommand};
use pravega_wire_protocol::wire_commands::{Encode, Replies, Requests};
use std::net::SocketAddr;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::info;

struct MockServer {
    address: SocketAddr,
    listener: TcpListener,
    read_event_size: usize,
}

impl MockServer {
    pub async fn with_size(read_event_size: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("local server");
        let address = listener.local_addr().unwrap();
        MockServer { address, listener, read_event_size }
    }

    pub async fn run_raw(mut self) {
        info!("MockServer::run_raw: BEGIN");

        // data chunk used to respond to ReadSegment
        let data_chunk = vec![0xAAu8; self.read_event_size];
        info!("MockServer::run_raw: created data_chunk");

        let event_data: Vec<u8> = Requests::Event(EventCommand {
            data: data_chunk,
        })
        .write_fields()
        .expect("Encoding event");
        info!("MockServer::run_raw: encoded Event");

        let segment_read_reply = Replies::SegmentRead(SegmentReadCommand {
            segment: "segment".to_string(),
            offset: 0,
            at_tail: false,
            end_of_segment: false,
            data: event_data.clone(),
            request_id: 0,
        })
        .write_fields()
        .expect("error while encoding segment read ");
        info!("MockServer::run_raw: encoded SegmentRead");

        info!("MockServer::run_raw: listening on {:?}", self.address);
        let (mut stream, addr) = self.listener.accept().await.expect("get incoming stream");
        info!("MockServer::run_raw: accepted connection from addr {:?}", addr);
        loop {
            info!("MockServer::run_raw: writing reply");
            stream
            .write_all(&segment_read_reply)
            .await
            .expect("Write segment read reply to client");
            info!("MockServer::run_raw: done writing reply");
        }
    }
}

fn main() {
    let _ = tracing_subscriber::fmt::try_init();
    info!("benchmark_tcp.main: BEGIN");
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let size: usize = 8*1024*1024;
    let mock_server = rt.block_on(MockServer::with_size(size));
    let addr = mock_server.address.clone();
    rt.spawn(async { MockServer::run_raw(mock_server).await });
    info!("benchmark_tcp.main: connecting to addr {:?}", addr);
    let mut stream = rt.block_on(TcpStream::connect(addr)).expect("connect");
    info!("benchmark_tcp.main: connected to addr {:?}", addr);
    let mut payload: Vec<u8> = vec![0; size as usize];
    let mut t0 = Instant::now();
    let mut num_bytes: u64 = 0;
    let mut warm = false;
    loop {
        rt.block_on(async {
            stream
            .read_exact(&mut payload[..])
            .await
            .expect("read payload from incoming stream");
        });
        num_bytes += payload.len() as u64;
        info!("benchmark_tcp.main: received {} bytes", payload.len());
        if warm {
            if t0.elapsed().as_secs_f64() > 4.0 {
                break;
            }
        } else {
            if t0.elapsed().as_secs_f64() > 2.0 {
                t0 = Instant::now();
                num_bytes = 0;
                warm = true;
                info!("benchmark_tcp.main: warmed up");
            }
        }
    }
    let dt = t0.elapsed().as_secs_f64();
    let megabytes_per_sec = (num_bytes as f64) * 1e-6 / dt;
    info!("benchmark_tcp.main: END; dt={}, megabytes_per_sec={}", dt, megabytes_per_sec);
}
