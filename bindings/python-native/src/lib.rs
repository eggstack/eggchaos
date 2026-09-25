//! PyO3 native embedding pilot: managed `Service` over `eggchaos-embed`.
//!
//! # FFI safety note
//!
//! The workspace forbids handwritten `unsafe` in all normal Rust crates.
//! This binding crate carries a crate-local `allow(unsafe_code)` for one
//! reason only: PyO3's `#[pymodule]`/`#[pyclass]`/`#[pymethods]` macros
//! expand to framework-owned FFI glue containing `unsafe` blocks. There
//! is no handwritten `unsafe` in this file (verified by inspection and
//! by `grep -rn "unsafe" bindings/python-native/src` returning only the
//! allow attribute below). Any future handwritten `unsafe` here stops the
//! milestone and requires a new ADR. See the M034 closure audit.
#![allow(unsafe_code)]

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use eggchaos_embed::{EmbedError, EmbeddedService};

fn convert_error(error: EmbedError) -> PyErr {
    match error {
        EmbedError::Validation(detail) => NativeValidationError::new_err(detail),
        EmbedError::NotFound(detail) => NativeNotFoundError::new_err(detail),
        EmbedError::Conflict(detail) => NativeConflictError::new_err(detail),
        EmbedError::Unsupported(detail) => NativeUnsupportedError::new_err(detail),
        EmbedError::Lifecycle(detail) => NativeLifecycleError::new_err(detail),
        EmbedError::Bind(detail) => NativeBindError::new_err(detail),
        EmbedError::Internal(detail) => NativeInternalError::new_err(detail),
    }
}

pyo3::create_exception!(eggchaos_native, NativeError, pyo3::exceptions::PyException);
pyo3::create_exception!(eggchaos_native, NativeValidationError, NativeError);
pyo3::create_exception!(eggchaos_native, NativeNotFoundError, NativeError);
pyo3::create_exception!(eggchaos_native, NativeConflictError, NativeError);
pyo3::create_exception!(eggchaos_native, NativeUnsupportedError, NativeError);
pyo3::create_exception!(eggchaos_native, NativeLifecycleError, NativeError);
pyo3::create_exception!(eggchaos_native, NativeBindError, NativeError);
pyo3::create_exception!(eggchaos_native, NativeInternalError, NativeError);

fn value_to_python(py: Python<'_>, value: serde_json::Value) -> PyResult<Py<PyAny>> {
    match value {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(flag) => Ok(flag.into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Number(number) => {
            if let Some(int) = number.as_u64() {
                Ok(int.into_pyobject(py)?.to_owned().into_any().unbind())
            } else if let Some(int) = number.as_i64() {
                Ok(int.into_pyobject(py)?.to_owned().into_any().unbind())
            } else if let Some(float) = number.as_f64() {
                Ok(float.into_pyobject(py)?.to_owned().into_any().unbind())
            } else {
                Err(PyValueError::new_err("unsupported JSON number"))
            }
        }
        serde_json::Value::String(text) => {
            Ok(text.into_pyobject(py)?.to_owned().into_any().unbind())
        }
        serde_json::Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(value_to_python(py, item)?)?;
            }
            Ok(list.into())
        }
        serde_json::Value::Object(map) => {
            let dict = PyDict::new(py);
            for (key, item) in map {
                dict.set_item(key, value_to_python(py, item)?)?;
            }
            Ok(dict.into())
        }
    }
}

fn to_python(py: Python<'_>, value: impl serde::Serialize) -> PyResult<Py<PyAny>> {
    let json = serde_json::to_value(value)
        .map_err(|error| NativeInternalError::new_err(format!("serialization: {error}")))?;
    value_to_python(py, json)
}

/// A fault description: identity plus a native kind document.
#[pyclass(from_py_object)]
#[derive(Debug, Clone)]
struct Fault {
    direction: String,
    id: String,
    probability: Option<f64>,
    kind: serde_json::Value,
}

#[pymethods]
impl Fault {
    #[staticmethod]
    #[pyo3(signature = (id, *, direction, delay_ns, jitter_ns=0, max_buffer_bytes=None, probability=None))]
    fn latency(
        id: String,
        direction: String,
        delay_ns: u64,
        jitter_ns: u64,
        max_buffer_bytes: Option<u64>,
        probability: Option<f64>,
    ) -> Self {
        let mut kind = serde_json::json!({"type": "latency", "delay_ns": delay_ns});
        if jitter_ns != 0 {
            kind["jitter_ns"] = jitter_ns.into();
        }
        if let Some(bytes) = max_buffer_bytes {
            kind["max_buffer_bytes"] = bytes.into();
        }
        Self {
            direction,
            id,
            probability,
            kind,
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, bytes_per_second=None, burst_bytes=None, probability=None))]
    fn bandwidth(
        id: String,
        direction: String,
        bytes_per_second: Option<u64>,
        burst_bytes: Option<u64>,
        probability: Option<f64>,
    ) -> Self {
        let mut kind = serde_json::json!({"type": "bandwidth"});
        if let Some(rate) = bytes_per_second {
            kind["bytes_per_second"] = rate.into();
        }
        if let Some(burst) = burst_bytes {
            kind["burst_bytes"] = burst.into();
        }
        Self {
            direction,
            id,
            probability,
            kind,
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, close_after_ns=None, probability=None))]
    fn blackhole(
        id: String,
        direction: String,
        close_after_ns: Option<u64>,
        probability: Option<f64>,
    ) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "blackhole", "close_after_ns": close_after_ns}),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, bytes=None, probability=None))]
    fn limit_data(
        id: String,
        direction: String,
        bytes: Option<u64>,
        probability: Option<f64>,
    ) -> Self {
        let mut kind = serde_json::json!({"type": "limit-data"});
        if let Some(count) = bytes {
            kind["bytes"] = count.into();
        }
        Self {
            direction,
            id,
            probability,
            kind,
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, delay_ns, probability=None))]
    fn slow_close(id: String, direction: String, delay_ns: u64, probability: Option<f64>) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "slow-close", "delay_ns": delay_ns}),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, average_size=None, variation=0, delay_ns=0, probability=None))]
    fn slice(
        id: String,
        direction: String,
        average_size: Option<u64>,
        variation: u64,
        delay_ns: u64,
        probability: Option<f64>,
    ) -> Self {
        let mut kind =
            serde_json::json!({"type": "slice", "variation": variation, "delay_ns": delay_ns});
        if let Some(size) = average_size {
            kind["average_size"] = size.into();
        }
        Self {
            direction,
            id,
            probability,
            kind,
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, after_ns=0, hard_reset=false, probability=None))]
    fn disconnect(
        id: String,
        direction: String,
        after_ns: u64,
        hard_reset: bool,
        probability: Option<f64>,
    ) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "disconnect", "after_ns": after_ns, "hard_reset": hard_reset}),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, delay_ns, jitter_ns=0, probability=None))]
    fn datagram_delay(
        id: String,
        direction: String,
        delay_ns: u64,
        jitter_ns: u64,
        probability: Option<f64>,
    ) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "delay", "delay_ns": delay_ns, "jitter_ns": jitter_ns}),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, probability=None))]
    fn datagram_loss(id: String, direction: String, probability: Option<f64>) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "loss"}),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, additional_copies, probability=None))]
    fn datagram_duplicate(
        id: String,
        direction: String,
        additional_copies: u8,
        probability: Option<f64>,
    ) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "duplicate", "additional_copies": additional_copies}),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, hold_ns, probability=None))]
    fn datagram_reorder(
        id: String,
        direction: String,
        hold_ns: u64,
        probability: Option<f64>,
    ) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "reorder", "hold_ns": hold_ns}),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, bytes, probability=None))]
    fn datagram_corrupt(
        id: String,
        direction: String,
        bytes: u64,
        probability: Option<f64>,
    ) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "payload-corrupt", "bytes": bytes}),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (id, *, direction, bytes_per_second, burst_bytes, probability=None))]
    fn datagram_bandwidth(
        id: String,
        direction: String,
        bytes_per_second: u64,
        burst_bytes: u64,
        probability: Option<f64>,
    ) -> Self {
        Self {
            direction,
            id,
            probability,
            kind: serde_json::json!({"type": "bandwidth", "bytes_per_second": bytes_per_second, "burst_bytes": burst_bytes}),
        }
    }
}

/// An embedded eggchaos service running in this process.
///
/// Managed lifecycle: use as a context manager (or call `close`
/// explicitly). All blocking control releases the GIL.
#[pyclass]
struct Service {
    inner: Option<EmbeddedService>,
}

#[pymethods]
impl Service {
    #[new]
    #[pyo3(signature = (*, seed=0))]
    fn new(seed: u64) -> PyResult<Self> {
        let inner = EmbeddedService::start(eggchaos_embed::EmbedOptions {
            seed,
            ..Default::default()
        })
        .map_err(convert_error)?;
        Ok(Self { inner: Some(inner) })
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (exc_type=None, exc=None, tb=None))]
    fn __exit__(
        &mut self,
        exc_type: Option<Py<PyAny>>,
        exc: Option<Py<PyAny>>,
        tb: Option<Py<PyAny>>,
    ) -> PyResult<bool> {
        let _ = (exc_type, exc, tb);
        self.close();
        Ok(false)
    }

    /// Idempotent close: shut down listeners and join supervised tasks.
    fn close(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.shutdown();
        }
    }

    fn closed(&self) -> bool {
        self.inner.as_ref().is_none_or(EmbeddedService::is_closed)
    }

    fn health<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py.detach(|| service.health()).map_err(convert_error)?;
        to_python(py, out)
    }

    fn version<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py.detach(|| service.version()).map_err(convert_error)?;
        to_python(py, out)
    }

    fn metrics_text<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py
            .detach(|| service.metrics_text())
            .map_err(convert_error)?;
        to_python(py, out)
    }

    fn reset<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py.detach(|| service.reset()).map_err(convert_error)?;
        to_python(py, out)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (*, name, listen, upstream, enabled=true, max_connections=None, connect_timeout_ms=None, seed=None))]
    fn create_proxy<'py>(
        &self,
        py: Python<'py>,
        name: String,
        listen: String,
        upstream: String,
        enabled: bool,
        max_connections: Option<usize>,
        connect_timeout_ms: Option<u64>,
        seed: Option<u64>,
    ) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let mut body = serde_json::json!({
            "name": name, "listen": listen, "upstream": upstream, "enabled": enabled,
        });
        if let Some(limit) = max_connections {
            body["max_connections"] = limit.into();
        }
        if let Some(timeout) = connect_timeout_ms {
            body["connect_timeout_ms"] = timeout.into();
        }
        if let Some(seed) = seed {
            body["seed"] = seed.into();
        }
        let request: eggchaos_protocol::NativeProxyRequestV1 = serde_json::from_value(body)
            .map_err(|error| NativeValidationError::new_err(error.to_string()))?;
        let (view, generation) = py
            .detach(|| service.create_proxy(request))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"proxy": view, "generation": generation}),
        )
    }

    fn list_proxies<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py
            .detach(|| service.list_proxies())
            .map_err(convert_error)?;
        to_python(py, out)
    }

    fn get_proxy<'py>(&self, py: Python<'py>, name: String) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py
            .detach(|| service.get_proxy(&name))
            .map_err(convert_error)?;
        to_python(py, out)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (name, *, listen=None, upstream=None, enabled=None, max_connections=None, connect_timeout_ms=None))]
    fn patch_proxy<'py>(
        &self,
        py: Python<'py>,
        name: String,
        listen: Option<String>,
        upstream: Option<String>,
        enabled: Option<bool>,
        max_connections: Option<usize>,
        connect_timeout_ms: Option<u64>,
    ) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let mut body = serde_json::json!({});
        if let Some(value) = listen {
            body["listen"] = value.into();
        }
        if let Some(value) = upstream {
            body["upstream"] = value.into();
        }
        if let Some(value) = enabled {
            body["enabled"] = value.into();
        }
        if let Some(value) = max_connections {
            body["max_connections"] = value.into();
        }
        if let Some(value) = connect_timeout_ms {
            body["connect_timeout_ms"] = value.into();
        }
        let patch: eggchaos_protocol::NativeProxyPatchV1 = serde_json::from_value(body)
            .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
        let (view, generation) = py
            .detach(|| service.patch_proxy(&name, patch))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"proxy": view, "generation": generation}),
        )
    }

    fn delete_proxy<'py>(&self, py: Python<'py>, name: String) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let generation = py
            .detach(|| service.delete_proxy(&name))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"generation": generation, "deleted": true}),
        )
    }

    fn set_fault<'py>(&self, py: Python<'py>, proxy: String, fault: &Fault) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let mut body = serde_json::json!({
            "direction": fault.direction, "id": fault.id, "kind": fault.kind,
        });
        if let Some(probability) = fault.probability {
            body["probability"] = probability.into();
        }
        let upsert: eggchaos_protocol::FaultUpsertV1 = serde_json::from_value(body)
            .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
        let (direction, view, generation) = py
            .detach(|| service.add_fault(&proxy, upsert))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"direction": direction.as_str(), "fault": view, "generation": generation}),
        )
    }

    fn list_faults<'py>(&self, py: Python<'py>, proxy: String) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let (upstream, downstream) = py
            .detach(|| service.list_faults(&proxy))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"upstream": upstream, "downstream": downstream}),
        )
    }

    fn get_fault<'py>(&self, py: Python<'py>, proxy: String, id: String) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let (direction, view) = py
            .detach(|| service.get_fault(&proxy, &id))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"direction": direction.as_str(), "fault": view}),
        )
    }

    #[pyo3(signature = (proxy, id, *, probability=None, kind=None))]
    fn patch_fault<'py>(
        &self,
        py: Python<'py>,
        proxy: String,
        id: String,
        probability: Option<f64>,
        kind: Option<&Fault>,
    ) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let mut body = serde_json::json!({});
        if let Some(value) = probability {
            body["probability"] = value.into();
        }
        if let Some(fault) = kind {
            body["kind"] = fault.kind.clone();
        }
        let patch: eggchaos_protocol::FaultPatchV1 = serde_json::from_value(body)
            .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
        let (direction, view, generation) = py
            .detach(|| service.patch_fault(&proxy, &id, patch))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"direction": direction.as_str(), "fault": view, "generation": generation}),
        )
    }

    fn remove_fault<'py>(&self, py: Python<'py>, proxy: String, id: String) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let generation = py
            .detach(|| service.remove_fault(&proxy, &id))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"generation": generation, "deleted": true}),
        )
    }

    fn connections<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py.detach(|| service.connections()).map_err(convert_error)?;
        to_python(py, out)
    }

    fn kill_connection<'py>(&self, py: Python<'py>, id: u64) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let terminated = py
            .detach(|| service.kill_connection(id))
            .map_err(convert_error)?;
        to_python(py, serde_json::json!({"id": id, "terminated": terminated}))
    }

    fn history<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py.detach(|| service.history()).map_err(convert_error)?;
        to_python(py, out)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (*, name, listen, upstream, max_associations=None, association_idle_timeout_ms=None, max_queued_datagrams=None, max_queued_bytes=None, max_datagram_size=None, seed=None))]
    fn create_datagram_proxy<'py>(
        &self,
        py: Python<'py>,
        name: String,
        listen: String,
        upstream: String,
        max_associations: Option<usize>,
        association_idle_timeout_ms: Option<u64>,
        max_queued_datagrams: Option<u64>,
        max_queued_bytes: Option<u64>,
        max_datagram_size: Option<u64>,
        seed: Option<u64>,
    ) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let mut body = serde_json::json!({"name": name, "listen": listen, "upstream": upstream});
        if let Some(value) = max_associations {
            body["max_associations"] = value.into();
        }
        if let Some(value) = association_idle_timeout_ms {
            body["association_idle_timeout_ms"] = value.into();
        }
        if let Some(value) = max_queued_datagrams {
            body["max_queued_datagrams"] = value.into();
        }
        if let Some(value) = max_queued_bytes {
            body["max_queued_bytes"] = value.into();
        }
        if let Some(value) = max_datagram_size {
            body["max_datagram_size"] = value.into();
        }
        if let Some(value) = seed {
            body["seed"] = value.into();
        }
        let request: eggchaos_protocol::NativeDatagramProxyRequestV1 = serde_json::from_value(body)
            .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
        let (view, generation) = py
            .detach(|| service.create_datagram_proxy(request))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"proxy": view, "generation": generation}),
        )
    }

    fn list_datagram_proxies<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py
            .detach(|| service.list_datagram_proxies())
            .map_err(convert_error)?;
        to_python(py, out)
    }

    fn get_datagram_proxy<'py>(&self, py: Python<'py>, name: String) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py
            .detach(|| service.get_datagram_proxy(&name))
            .map_err(convert_error)?;
        to_python(py, out)
    }

    fn delete_datagram_proxy<'py>(&self, py: Python<'py>, name: String) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let generation = py
            .detach(|| service.delete_datagram_proxy(&name))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"generation": generation, "deleted": true}),
        )
    }

    fn set_datagram_fault<'py>(
        &self,
        py: Python<'py>,
        proxy: String,
        fault: &Fault,
    ) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let mut body = serde_json::json!({
            "direction": fault.direction, "id": fault.id, "kind": fault.kind,
        });
        if let Some(probability) = fault.probability {
            body["probability"] = probability.into();
        }
        let upsert: eggchaos_protocol::DatagramFaultUpsertV1 = serde_json::from_value(body)
            .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
        let (direction, view, generation) = py
            .detach(|| service.add_datagram_fault(&proxy, upsert))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"direction": direction.as_str(), "fault": view, "generation": generation}),
        )
    }

    fn get_datagram_fault<'py>(
        &self,
        py: Python<'py>,
        proxy: String,
        id: String,
    ) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let (direction, view) = py
            .detach(|| service.get_datagram_fault(&proxy, &id))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"direction": direction.as_str(), "fault": view}),
        )
    }

    fn remove_datagram_fault<'py>(
        &self,
        py: Python<'py>,
        proxy: String,
        id: String,
    ) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let generation = py
            .detach(|| service.remove_datagram_fault(&proxy, &id))
            .map_err(convert_error)?;
        to_python(
            py,
            serde_json::json!({"generation": generation, "deleted": true}),
        )
    }

    fn datagram_associations<'py>(&self, py: Python<'py>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py
            .detach(|| service.datagram_associations())
            .map_err(convert_error)?;
        to_python(py, out)
    }

    fn scenario_apply<'py>(&self, py: Python<'py>, document: Py<PyAny>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let json: serde_json::Value = python_to_value(py, &document)?;
        let version = json.get("version").and_then(serde_json::Value::as_u64);
        if version == Some(2) {
            let dto: eggchaos_protocol::ScenarioScheduleV2Dto = serde_json::from_value(json)
                .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
            let out = py
                .detach(|| service.apply_schedule_v2(dto))
                .map_err(convert_error)?;
            to_python(py, eggchaos_embed::ScenarioRunView::V2(out))
        } else {
            let dto: eggchaos_protocol::ScenarioV1 = serde_json::from_value(json)
                .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
            let out = py
                .detach(|| service.apply_scenario_v1(dto))
                .map_err(convert_error)?;
            to_python(py, eggchaos_embed::ScenarioRunView::V1(out))
        }
    }

    fn schedule_validate<'py>(&self, py: Python<'py>, document: Py<PyAny>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let json: serde_json::Value = python_to_value(py, &document)?;
        let dto: eggchaos_protocol::ScenarioScheduleV2Dto = serde_json::from_value(json)
            .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
        let out = service.validate_schedule_v2(dto).map_err(convert_error)?;
        to_python(py, out)
    }

    fn schedule_compile<'py>(&self, py: Python<'py>, document: Py<PyAny>) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let json: serde_json::Value = python_to_value(py, &document)?;
        let dto: eggchaos_protocol::ScenarioScheduleV2Dto = serde_json::from_value(json)
            .map_err(|error| convert_error(EmbedError::Validation(error.to_string())))?;
        let out = service.compile_schedule_v2(dto).map_err(convert_error)?;
        to_python(py, out)
    }

    fn scenario_get<'py>(&self, py: Python<'py>, run_id: u64) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py
            .detach(|| service.get_scenario(run_id))
            .map_err(convert_error)?;
        to_python(py, out)
    }

    fn scenario_cancel<'py>(&self, py: Python<'py>, run_id: u64) -> PyResult<Py<PyAny>> {
        let service = require(&self.inner)?;
        let out = py
            .detach(|| service.cancel_scenario(run_id))
            .map_err(convert_error)?;
        to_python(py, out)
    }
}

fn require(inner: &Option<EmbeddedService>) -> PyResult<&EmbeddedService> {
    inner
        .as_ref()
        .filter(|service| !service.is_closed())
        .ok_or_else(|| convert_error(EmbedError::Lifecycle("service is closed".into())))
}

fn python_to_value(py: Python<'_>, object: &Py<PyAny>) -> PyResult<serde_json::Value> {
    use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString};
    let bound = object.bind(py);
    if bound.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(value) = bound.cast::<PyBool>() {
        return Ok(serde_json::Value::Bool(value.extract()?));
    }
    if let Ok(value) = bound.cast::<PyInt>() {
        if let Ok(int) = value.extract::<u64>() {
            return Ok(int.into());
        }
        if let Ok(int) = value.extract::<i64>() {
            return Ok(int.into());
        }
        return Err(PyValueError::new_err("integer out of range"));
    }
    if let Ok(value) = bound.cast::<PyFloat>() {
        let float: f64 = value.extract()?;
        return serde_json::json!(float)
            .as_f64()
            .map(serde_json::Value::from)
            .ok_or_else(|| PyValueError::new_err("non-finite float"));
    }
    if let Ok(value) = bound.cast::<PyString>() {
        return Ok(value.extract::<String>()?.into());
    }
    if let Ok(list) = bound.cast::<PyList>() {
        return list
            .iter()
            .map(|item| python_to_value(py, &item.into()))
            .collect::<PyResult<Vec<_>>>()
            .map(serde_json::Value::from);
    }
    if let Ok(tuple) = bound.cast::<pyo3::types::PyTuple>() {
        return tuple
            .iter()
            .map(|item| python_to_value(py, &item.into()))
            .collect::<PyResult<Vec<_>>>()
            .map(serde_json::Value::from);
    }
    if let Ok(dict) = bound.cast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (key, value) in dict.iter() {
            let key: String = key
                .extract()
                .map_err(|_| PyValueError::new_err("mapping keys must be strings"))?;
            map.insert(key, python_to_value(py, &value.into())?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    Err(PyValueError::new_err(format!(
        "unsupported value of type {}",
        bound.get_type().name()?
    )))
}

/// Native in-process eggchaos service for Python test harnesses.
#[pymodule]
fn eggchaos_native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Service>()?;
    module.add_class::<Fault>()?;
    module.add("NativeError", module.py().get_type::<NativeError>())?;
    module.add(
        "NativeValidationError",
        module.py().get_type::<NativeValidationError>(),
    )?;
    module.add(
        "NativeNotFoundError",
        module.py().get_type::<NativeNotFoundError>(),
    )?;
    module.add(
        "NativeConflictError",
        module.py().get_type::<NativeConflictError>(),
    )?;
    module.add(
        "NativeUnsupportedError",
        module.py().get_type::<NativeUnsupportedError>(),
    )?;
    module.add(
        "NativeLifecycleError",
        module.py().get_type::<NativeLifecycleError>(),
    )?;
    module.add("NativeBindError", module.py().get_type::<NativeBindError>())?;
    module.add(
        "NativeInternalError",
        module.py().get_type::<NativeInternalError>(),
    )?;
    Ok(())
}
