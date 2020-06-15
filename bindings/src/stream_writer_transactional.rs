//
// Copyright (c) Dell Inc., or its subsidiaries. All Rights Reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//

use pravega_rust_client_shared::ScopedStream;
cfg_if! {
    if #[cfg(feature = "python_binding")] {
        use pravega_client_rust::transaction::transactional_event_stream_writer::TransactionalEventStreamWriter;
        use pyo3::exceptions;
        use pyo3::prelude::*;
        use pyo3::PyResult;
        use pyo3::PyObjectProtocol;
        use crate::transaction::StreamTransaction;
        use pravega_rust_client_shared::TxId;
        use tokio::runtime::Handle;
    }
}

#[cfg(feature = "python_binding")]
#[pyclass]
#[derive(new)] // this ensures the python object cannot be created without the using StreamManager.
pub(crate) struct StreamTxnWriter {
    writer: TransactionalEventStreamWriter,
    handle: Handle,
    stream: ScopedStream,
}

#[cfg(feature = "python_binding")]
#[pymethods]
impl StreamTxnWriter {
    ///
    /// Create a new transaction.
    /// This returns a StreamTransaction which can be perform writes on the created transaction. It
    /// can also be used to perform commit() and abort() operations on the created transaction.
    ///
    #[cfg(feature = "python_binding")]
    #[text_signature = "($self)"]
    pub fn begin_txn(&mut self) -> PyResult<StreamTransaction> {
        let result = self.handle.block_on(self.writer.begin());
        match result {
            Ok(txn) => Ok(StreamTransaction::new(txn, self.handle.clone())),
            Err(e) => Err(exceptions::ValueError::py_err(format!("{:?}", e))),
        }
    }

    ///
    /// Get a StreamTransaction for a given transaction id.
    ///
    #[cfg(feature = "python_binding")]
    #[text_signature = "($self, txn_id)"]
    pub fn get_txn(&mut self, txn_id: u128) -> PyResult<StreamTransaction> {
        println!("Writing a single event for a given routing key");
        let result = self.handle.block_on(self.writer.get_txn(TxId(txn_id)));

        match result {
            Ok(txn) => Ok(StreamTransaction::new(txn, self.handle.clone())),
            Err(e) => Err(exceptions::ValueError::py_err(format!("{:?}", e))),
        }
    }

    /// Returns the facet string representation.
    fn to_str(&self) -> String {
        format!("Stream: {:?} ", self.stream)
    }
}

///
/// Refer https://docs.python.org/3/reference/datamodel.html#basic-customization
/// This function will be called by the repr() built-in function to compute the “official” string
/// representation of an Python object.
///
#[cfg(feature = "python_binding")]
#[pyproto]
impl PyObjectProtocol for StreamTxnWriter {
    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("StreamTxnWriter({})", self.to_str()))
    }
}
