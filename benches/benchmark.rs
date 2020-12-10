//
// Copyright (c) Dell Inc., or its subsidiaries. All Rights Reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};

use byteorder::BigEndian;
use pravega_client_rust::client_factory::ClientFactory;
use pravega_client_rust::error::SegmentWriterError;
use pravega_client_rust::event_reader::EventReader;
use pravega_client_rust::event_stream_writer::EventStreamWriter;
use pravega_controller_client::ControllerClient;
use pravega_rust_client_config::connection_type::{ConnectionType, MockType};
use pravega_rust_client_config::{ClientConfig, ClientConfigBuilder};
use pravega_rust_client_shared::*;
use pravega_wire_protocol::{client_connection::{LENGTH_FIELD_LENGTH, LENGTH_FIELD_OFFSET}, commands::ReadSegmentCommand};
use pravega_wire_protocol::commands::{
    AppendSetupCommand, DataAppendedCommand, EventCommand, SegmentCreatedCommand, SegmentReadCommand,
    TableEntries, TableEntriesDeltaReadCommand, TableEntriesUpdatedCommand, TYPE_PLUS_LENGTH_SIZE,
};
use pravega_wire_protocol::wire_commands::{Decode, Encode, Replies, Requests};
use std::io::Cursor;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::info;

static EVENT_NUM: usize = 10000;
static EVENT_SIZE: usize = 100;
const READ_EVENT_SIZE_BYTES: usize = 1 * 1024 * 1024; //100 KB event.

struct MockServer {
    address: SocketAddr,
    listener: TcpListener,
    read_event_size: usize,
}

impl MockServer {
    pub async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("local server");
        let address = listener.local_addr().unwrap();
        let read_event_size = READ_EVENT_SIZE_BYTES;
        MockServer { address, listener, read_event_size }
    }

    pub async fn with_size(read_event_size: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("local server");
        let address = listener.local_addr().unwrap();
        MockServer { address, listener, read_event_size }
    }

    pub async fn run(mut self) {
        info!("run: listening on {:?}", self.address);
        // data chunk used to respond to ReadSegment
        let data_chunk = vec![0xAAu8; self.read_event_size];
        let event_data: Vec<u8> = Requests::Event(EventCommand {
            data: data_chunk,
        })
        .write_fields()
        .expect("Encoding event");
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

        let (mut stream, addr) = self.listener.accept().await.expect("get incoming stream");
        info!("run: accepted connection from addr {:?}", addr);
        loop {
            let mut header: Vec<u8> = vec![0; LENGTH_FIELD_OFFSET as usize + LENGTH_FIELD_LENGTH as usize];
            stream
                .read_exact(&mut header[..])
                .await
                .expect("read header from incoming stream");
            let mut rdr = Cursor::new(&header[4..8]);
            let payload_length =
                byteorder::ReadBytesExt::read_u32::<BigEndian>(&mut rdr).expect("exact size");
            let mut payload: Vec<u8> = vec![0; payload_length as usize];
            stream
                .read_exact(&mut payload[..])
                .await
                .expect("read payload from incoming stream");
            let concatenated = [&header[..], &payload[..]].concat();
            let request: Requests = Requests::read_from(&concatenated).expect("decode wirecommand");
            info!("run: thread={:?}, request={:?}", thread::current().id(), request);
            match request {
                Requests::Hello(cmd) => {
                    let reply = Replies::Hello(cmd).write_fields().expect("encode reply");
                    stream
                        .write_all(&reply)
                        .await
                        .expect("write reply back to client");
                }
                Requests::CreateTableSegment(cmd) => {
                    let reply = Replies::SegmentCreated(SegmentCreatedCommand {
                        request_id: cmd.request_id,
                        segment: cmd.segment,
                    })
                    .write_fields()
                    .expect("encode reply");
                    stream
                        .write_all(&reply)
                        .await
                        .expect("write reply back to client");
                }
                Requests::SetupAppend(cmd) => {
                    let reply = Replies::AppendSetup(AppendSetupCommand {
                        request_id: cmd.request_id,
                        segment: cmd.segment,
                        writer_id: cmd.writer_id,
                        last_event_number: -9223372036854775808, // when there is no previous event in this segment
                    })
                    .write_fields()
                    .expect("encode reply");
                    stream
                        .write_all(&reply)
                        .await
                        .expect("write reply back to client");
                }
                Requests::AppendBlockEnd(cmd) => {
                    let reply = Replies::DataAppended(DataAppendedCommand {
                        writer_id: cmd.writer_id,
                        event_number: cmd.last_event_number,
                        previous_event_number: 0, //not used in event stream writer
                        request_id: cmd.request_id,
                        current_segment_write_offset: 0, //not used in event stream writer
                    })
                    .write_fields()
                    .expect("encode reply");
                    stream
                        .write_all(&reply)
                        .await
                        .expect("write reply back to client");
                }
                Requests::ReadSegment(cmd) => {
                    // let reply = Replies::SegmentRead(SegmentReadCommand {
                    //     segment: cmd.segment,
                    //     offset: cmd.offset,
                    //     at_tail: false,
                    //     end_of_segment: false,
                    //     data: event_data.clone(),
                    //     request_id: cmd.request_id,
                    // })
                    // .write_fields()
                    // .expect("error while encoding segment read ");
                    let reply = &segment_read_reply;
                    stream
                        .write_all(reply)
                        .await
                        .expect("Write segment read reply to client");
                }
                // Send a mock response for table entry updates.
                Requests::UpdateTableEntries(cmd) => {
                    let new_versions: Vec<i64> = cmd
                        .table_entries
                        .entries
                        .iter()
                        .map(|(k, _v)| k.key_version + 1)
                        .collect();
                    let reply = Replies::TableEntriesUpdated(TableEntriesUpdatedCommand {
                        request_id: 0,
                        updated_versions: new_versions,
                    })
                    .write_fields()
                    .expect("error while encoding TableEntriesUpdated");
                    stream
                        .write_all(&reply)
                        .await
                        .expect("Error while sending TableEntriesUpdate");
                }
                // This ensures the local state of the reader group state is treated as the latest.
                Requests::ReadTableEntriesDelta(cmd) => {
                    let reply = Replies::TableEntriesDeltaRead(TableEntriesDeltaReadCommand {
                        request_id: cmd.request_id,
                        segment: cmd.segment,
                        entries: TableEntries { entries: vec![] }, // no new updates.
                        should_clear: false,
                        reached_end: false,
                        last_position: cmd.from_position,
                    })
                    .write_fields()
                    .expect("Error while encoding TableEntriesDeltaRead");
                    stream
                        .write_all(&reply)
                        .await
                        .expect("Error while sending DeltaRead");
                }
                _ => {
                    panic!("unsupported request {:?}", request);
                }
            }
        }
    }

    pub async fn run_raw(mut self) {
        info!("MockServer::run_raw: listening on {:?}", self.address);
        // data chunk used to respond to ReadSegment
        let data_chunk = vec![0xAAu8; self.read_event_size];
        let event_data: Vec<u8> = Requests::Event(EventCommand {
            data: data_chunk,
        })
        .write_fields()
        .expect("Encoding event");
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

// Read a segment slice and consume events from the slice.
async fn run_reader(reader: &mut EventReader, last_offset: &mut i64) {
    if let Some(mut slice) = reader.acquire_segment().await {
        while let Some(e) = slice.next() {
            // validate offset in the segment.
            if *last_offset == -1i64 {
                assert_eq!(0, e.offset_in_segment);
            } else {
                assert_eq!(
                    READ_EVENT_SIZE_BYTES + 2 * TYPE_PLUS_LENGTH_SIZE as usize,
                    (e.offset_in_segment - *last_offset) as usize
                );
            }
            // validate the event read length
            assert_eq!(
                READ_EVENT_SIZE_BYTES + TYPE_PLUS_LENGTH_SIZE as usize,
                e.value.len()
            );
            *last_offset = e.offset_in_segment;
        }
    } else {
        assert!(false, "No slice acquired");
    }
}

// This benchmark test uses a mock server that replies ok to any requests instantly. It involves
// kernel latency.
fn read_mock_server(c: &mut Criterion) {
    let _ = tracing_subscriber::fmt::try_init();
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let mock_server = rt.block_on(MockServer::new());
    let config = ClientConfigBuilder::default()
        .controller_uri(mock_server.address)
        .mock(true)
        .build()
        .expect("creating config");
    rt.spawn(async { MockServer::run(mock_server).await });
    let mut reader = rt.block_on(setup_reader(config));
    info!("start reader with mock server performance testing");
    let mut last_offset: i64 = -1;
    c.bench_function("read 100KB mock server", |b| {
        b.iter(|| {
            info!("read_mock_server: iter: last_offset={}", last_offset);
            rt.block_on(run_reader(&mut reader, &mut last_offset));
        });
    });
    println!("reader performance testing finished");
    info!("last_offset={}", last_offset);
}

// Send a ReadSegment request on a TcpStream and receive a reply.
async fn run_read_mock_client(stream: &mut TcpStream, last_offset: &mut i64, read_event_size: usize) {
    info!("run_read_mock_client: thread={:?}, last_offset={}", thread::current().id(), last_offset);
    // Send request.
    let request = Requests::ReadSegment(ReadSegmentCommand {
        segment: "segment0".to_string(),
        offset: *last_offset,
        suggested_length: read_event_size as i32,
        delegation_token: "".to_string(),
        request_id: 0,
    })
    .write_fields()
    .expect("error while encoding ReadSegment");
    stream
        .write_all(&request)
        .await
        .expect("Error while sending ReadSegment");
    // Read response.
    let mut header: Vec<u8> = vec![0; LENGTH_FIELD_OFFSET as usize + LENGTH_FIELD_LENGTH as usize];
    stream
        .read_exact(&mut header[..])
        .await
        .expect("read header from incoming stream");
    let mut rdr = Cursor::new(&header[4..8]);
    let payload_length =
        byteorder::ReadBytesExt::read_u32::<BigEndian>(&mut rdr).expect("exact size");
    let mut payload: Vec<u8> = vec![0; payload_length as usize];
    stream
        .read_exact(&mut payload[..])
        .await
        .expect("read payload from incoming stream");
    let concatenated = [&header[..], &payload[..]].concat();
    let reply: Replies = Replies::read_from(&concatenated).expect("decode wirecommand");
    // info!("run_read_mock_client: reply={:?}", reply);
    match reply {
        Replies::SegmentRead(cmd) => {
            info!("run_read_mock_client: received {} bytes for offset {}", cmd.data.len(), cmd.offset);
        }
        _ => {
            panic!("unsupported reply {:?}", reply);
        }
    }
    *last_offset += read_event_size as i64;
}

// Benchmark reads using a mock server and mock client.
// This tests the TCP connection through the loopback adapter and serialization.
fn benchmark_read_mock_server_mock_client(c: &mut Criterion) {
    let _ = tracing_subscriber::fmt::try_init();
    info!("benchmark_read_mock_server_mock_client: BEGIN");
    let mut group = c.benchmark_group("benchmark_read_mock_server_mock_client");
    for size in [100*1024*1024].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Note that initializing the mock server will be included in the benchmark time.
            info!("benchmark_read_mock_server_mock_client: bench_with_input: BEGIN");
            let mut rt = tokio::runtime::Runtime::new().unwrap();
            let mock_server = rt.block_on(MockServer::with_size(size));
            let addr = mock_server.address.clone();
            rt.spawn(async { MockServer::run_raw(mock_server).await });
            let mut stream = rt.block_on(TcpStream::connect(addr)).expect("connect");
            let mut last_offset: i64 = -1;
            info!("benchmark_read_mock_server_mock_client: connected to addr {:?}", addr);
            b.iter(|| {
                rt.block_on(run_read_mock_client(&mut stream, &mut last_offset, size));
            });
            info!("benchmark_read_mock_server_mock_client: bench_with_input: END");
        });
    }
    group.finish();
    info!("benchmark_read_mock_server_mock_client: END");
}

// This benchmark test uses a mock server that replies ok to any requests instantly. It involves
// kernel latency.
fn mock_server(c: &mut Criterion) {
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let mock_server = rt.block_on(MockServer::new());
    let config = ClientConfigBuilder::default()
        .controller_uri(mock_server.address)
        .mock(true)
        .build()
        .expect("creating config");
    let mut writer = rt.block_on(set_up(config));
    rt.spawn(async { MockServer::run(mock_server).await });
    let _ = tracing_subscriber::fmt::try_init();
    info!("start mock server performance testing");
    c.bench_function("mock server", |b| {
        b.iter(|| {
            rt.block_on(run(&mut writer));
        });
    });
    info!("mock server performance testing finished");
}

// This benchmark test uses a mock server that replies ok to any requests instantly. It involves
// kernel latency. It does not wait for reply.
fn mock_server_no_block(c: &mut Criterion) {
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let mock_server = rt.block_on(MockServer::new());
    let config = ClientConfigBuilder::default()
        .controller_uri(mock_server.address)
        .mock(true)
        .build()
        .expect("creating config");
    let mut writer = rt.block_on(set_up(config));
    rt.spawn(async { MockServer::run(mock_server).await });
    let _ = tracing_subscriber::fmt::try_init();
    info!("start mock server(no block) performance testing");
    c.bench_function("mock server(no block)", |b| {
        b.iter(|| {
            rt.block_on(run_no_block(&mut writer));
        });
    });
    info!("mock server(no block) performance testing finished");
}

// This benchmark test uses a mock connection that replies ok to any requests instantly. It does not
// involve kernel latency.
fn mock_connection(c: &mut Criterion) {
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let config = ClientConfigBuilder::default()
        .controller_uri("127.0.0.1:9090".parse::<SocketAddr>().unwrap())
        .mock(true)
        .connection_type(ConnectionType::Mock(MockType::Happy))
        .build()
        .expect("creating config");
    let mut writer = rt.block_on(set_up(config));
    let _ = tracing_subscriber::fmt::try_init();
    info!("start mock connection performance testing");
    c.bench_function("mock connection", |b| {
        b.iter(|| {
            rt.block_on(run(&mut writer));
        });
    });
    info!("mock server connection testing finished");
}

// This benchmark test uses a mock connection that replies ok to any requests instantly. It does not
// involve kernel latency. It does not wait for reply.
fn mock_connection_no_block(c: &mut Criterion) {
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let config = ClientConfigBuilder::default()
        .controller_uri("127.0.0.1:9090".parse::<SocketAddr>().unwrap())
        .mock(true)
        .connection_type(ConnectionType::Mock(MockType::Happy))
        .build()
        .expect("creating config");
    let mut writer = rt.block_on(set_up(config));
    let _ = tracing_subscriber::fmt::try_init();
    info!("start mock connection(no block) performance testing");
    c.bench_function("mock connection(no block)", |b| {
        b.iter(|| {
            rt.block_on(run_no_block(&mut writer));
        });
    });
    info!("mock server connection(no block) testing finished");
}

// helper functions
async fn set_up(config: ClientConfig) -> EventStreamWriter {
    let scope_name: Scope = Scope::from("testWriterPerf".to_string());
    let stream_name = Stream::from("testWriterPerf".to_string());
    let client_factory = ClientFactory::new(config.clone());
    let controller_client = client_factory.get_controller_client();
    create_scope_stream(controller_client, &scope_name, &stream_name, 1).await;
    let scoped_stream = ScopedStream {
        scope: scope_name.clone(),
        stream: stream_name.clone(),
    };
    client_factory.create_event_stream_writer(scoped_stream)
}

async fn setup_reader(config: ClientConfig) -> EventReader {
    let scope_name: Scope = Scope::from("testReaderPerf".to_string());
    let stream_name = Stream::from("testReaderPerf".to_string());
    let client_factory = ClientFactory::new(config.clone());
    let controller_client = client_factory.get_controller_client();
    create_scope_stream(controller_client, &scope_name, &stream_name, 1).await;
    let scoped_stream = ScopedStream {
        scope: scope_name.clone(),
        stream: stream_name.clone(),
    };
    let reader_group = client_factory
        .create_reader_group("rg1".to_string(), scoped_stream)
        .await;

    let reader = reader_group.create_reader("r1".to_string()).await;
    reader
}

async fn create_scope_stream(
    controller_client: &dyn ControllerClient,
    scope_name: &Scope,
    stream_name: &Stream,
    segment_number: i32,
) {
    controller_client
        .create_scope(scope_name)
        .await
        .expect("create scope");
    info!("Scope created");
    let request = StreamConfiguration {
        scoped_stream: ScopedStream {
            scope: scope_name.clone(),
            stream: stream_name.clone(),
        },
        scaling: Scaling {
            scale_type: ScaleType::FixedNumSegments,
            target_rate: 0,
            scale_factor: 0,
            min_num_segments: segment_number,
        },
        retention: Retention {
            retention_type: RetentionType::None,
            retention_param: 0,
        },
    };
    controller_client
        .create_stream(&request)
        .await
        .expect("create stream");
    info!("Stream created");
}

// run sends request to server and wait for the reply
async fn run(writer: &mut EventStreamWriter) {
    let mut receivers = vec![];
    for _i in 0..EVENT_NUM {
        let rx = writer.write_event(vec![0; EVENT_SIZE]).await;
        receivers.push(rx);
    }
    assert_eq!(receivers.len(), EVENT_NUM);

    for rx in receivers {
        let reply: Result<(), SegmentWriterError> = rx.await.expect("wait for result from oneshot");
        assert_eq!(reply.is_ok(), true);
    }
}

// run no block sends request to server and does not wait for the reply
async fn run_no_block(writer: &mut EventStreamWriter) {
    let mut receivers = vec![];
    for _i in 0..EVENT_NUM {
        let rx = writer.write_event(vec![0; EVENT_SIZE]).await;
        receivers.push(rx);
    }
    assert_eq!(receivers.len(), EVENT_NUM);
}

criterion_group! {
    name = performance;
    config = Criterion::default().sample_size(10);
    targets = mock_server,mock_server_no_block,mock_connection,mock_connection_no_block
}
criterion_group! {
    name = reader_performance;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(10));
    // targets = read_mock_server;
    targets = benchmark_read_mock_server_mock_client
}
criterion_main!(reader_performance);
