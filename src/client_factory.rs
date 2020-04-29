//
// Copyright (c) Dell Inc., or its subsidiaries. All Rights Reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//

use std::net::SocketAddr;

use pravega_connection_pool::connection_pool::ConnectionPool;
use pravega_controller_client::{ControllerClient, ControllerClientImpl};
use pravega_rust_client_shared::{ScopedSegment, ScopedStream};
use pravega_wire_protocol::client_config::ClientConfig;
use pravega_wire_protocol::connection_factory::{ConnectionFactory, SegmentConnectionManager};

use crate::event_stream_writer::EventStreamWriter;
use crate::raw_client::RawClientImpl;
use crate::segment_reader::AsyncSegmentReaderImpl;
use crate::setup_logger;
use crate::tablemap::TableMap;
use std::sync::Arc;

pub struct ClientFactory(Arc<ClientFactoryInternal>);

pub struct ClientFactoryInternal {
    connection_pool: ConnectionPool<SegmentConnectionManager>,
    controller_client: ControllerClientImpl,
}

impl ClientFactory {
    pub fn new(config: ClientConfig) -> ClientFactory {
        let _ = setup_logger(); //Ignore failure
        let cf = ConnectionFactory::create(config.connection_type);
        let pool = ConnectionPool::new(SegmentConnectionManager::new(cf, config.max_connections_in_pool));
        let controller = ControllerClientImpl::new(config);
        ClientFactory(Arc::new(ClientFactoryInternal {
            connection_pool: pool,
            controller_client: controller,
        }))
    }

    #[allow(clippy::needless_lifetimes)] //Normally the compiler could infer lifetimes but async is throwing it for a loop.
    pub async fn create_async_event_reader<'a>(
        &'a self,
        segment: ScopedSegment,
    ) -> AsyncSegmentReaderImpl<'a> {
        AsyncSegmentReaderImpl::init(segment, &self.0).await
    }

    #[allow(clippy::needless_lifetimes)] //Normally the compiler could infer lifetimes but async is throwing it for a loop.
    pub async fn create_table_map<'a>(&'a self, name: String) -> TableMap<'a> {
        TableMap::new(name, &self.0)
            .await
            .expect("Failed to create Table map")
    }

    pub async fn create_raw_client(&self, segment: &ScopedSegment) -> RawClientImpl<'_> {
        let endpoint = self
            .0
            .controller_client
            .get_endpoint_for_segment(segment)
            .await
            .expect("get endpoint for segment")
            .parse::<SocketAddr>()
            .expect("convert to socketaddr");
        self.0.create_raw_client(endpoint)
    }

    pub fn create_event_stream_writer(
        &self,
        stream: ScopedStream,
        config: ClientConfig,
    ) -> EventStreamWriter {
        EventStreamWriter::new(stream, config, self.0.clone())
    }

    pub fn get_controller_client(&self) -> &dyn ControllerClient {
        &self.0.controller_client
    }
}

impl ClientFactoryInternal {
    pub(crate) fn create_raw_client(&self, endpoint: SocketAddr) -> RawClientImpl {
        RawClientImpl::new(&self.connection_pool, endpoint)
    }

    pub(crate) fn get_connection_pool(&self) -> &ConnectionPool<SegmentConnectionManager> {
        &self.connection_pool
    }

    pub(crate) fn get_controller_client(&self) -> &dyn ControllerClient {
        &self.controller_client
    }
}