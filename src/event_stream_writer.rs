//
// Copyright (c) Dell Inc., or its subsidiaries. All Rights Reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//

use std::sync::Arc;

use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::segment_writer::{AppendEvent, Incoming, Reactor, SegmentSelector};
use pravega_rust_client_shared::*;
use pravega_wire_protocol::client_config::ClientConfig;

use crate::client_factory::ClientFactoryInternal;
use crate::error::*;

/// EventStreamWriter contains a writer id and a mpsc sender which is used to send Event
/// to the StreamWriter
pub struct EventStreamWriter {
    writer_id: Uuid,
    sender: Sender<Incoming>,
}

impl EventStreamWriter {
    const CHANNEL_CAPACITY: usize = 100;

    pub(crate) fn new(
        stream: ScopedStream,
        config: ClientConfig,
        factory: Arc<ClientFactoryInternal>,
    ) -> Self {
        let (tx, rx) = channel(EventStreamWriter::CHANNEL_CAPACITY);
        let handle = factory.get_runtime_handle();

        let selector = SegmentSelector::new(stream, tx.clone(), config, factory.clone());
        let reactor = Reactor::new(rx, selector, factory);
        // tokio::spawn is tied to the factory runtime.
        handle.enter(|| tokio::spawn(Reactor::run(reactor)));
        EventStreamWriter {
            writer_id: Uuid::new_v4(),
            sender: tx,
        }
    }

    pub async fn write_event(&mut self, event: Vec<u8>) -> oneshot::Receiver<Result<(), SegmentWriter>> {
        let (tx, rx) = oneshot::channel();
        let append_event = Incoming::AppendEvent(AppendEvent::new(true, event, None, tx));
        self.writer_event_internal().await
    }

    pub async fn write_event_by_routing_key(
        &mut self,
        routing_key: String,
        event: Vec<u8>,
    ) -> oneshot::Receiver<Result<(), SegmentWriter>> {
        let (tx, rx) = oneshot::channel();
        let append_event = Incoming::AppendEvent(AppendEvent::new(true, event, Some(routing_key), tx));
        self.writer_event_internal().await
    }

    async fn writer_event_internal(&mut self) -> oneshot::Receiver<Result<(), SegmentWriter>> {
        if let Err(_e) = self.sender.send(append_event).await {
            let (tx_error, rx_error) = oneshot::channel();
            tx_error
                .send(Err(SegmentWriter::SendToProcessor {}))
                .expect("send error");
            rx_error
        } else {
            rx
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::runtime::Runtime;

    use super::*;
    use crate::segment_writer::PendingEvent;

    #[test]
    fn test_pending_event() {
        // test with legal event size
        let (tx, _rx) = oneshot::channel();
        let data = vec![];
        let routing_key = None;

        let event = PendingEvent::without_header(routing_key, data, tx).expect("create pending event");
        assert!(event.is_empty());

        // test with illegal event size
        let (tx, rx) = oneshot::channel();
        let data = vec![0; (PendingEvent::MAX_WRITE_SIZE + 1) as usize];
        let routing_key = None;

        let event = PendingEvent::without_header(routing_key, data, tx);
        assert!(event.is_none());

        let mut rt = Runtime::new().expect("get runtime");
        let reply = rt.block_on(rx).expect("get reply");
        assert!(reply.is_err());
    }
}
