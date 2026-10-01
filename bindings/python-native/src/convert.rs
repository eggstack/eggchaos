//! Pure conversion, validation, and lifecycle helpers for the binding.
//!
//! This module contains no PyO3 macro expansions, so the compiler
//! enforces it as unsafe-free (see the crate-root FFI safety note).
#![deny(unsafe_code)]

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use eggchaos_embed::EmbedError;

use crate::bridge::{
    NativeBindError, NativeConflictError, NativeInternalError, NativeLifecycleError,
    NativeNotFoundError, NativeUnsupportedError, NativeValidationError,
};

/// Map an embed-layer failure to the matching Python exception type.
pub(crate) fn convert_error(error: EmbedError) -> PyErr {
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

/// Convert a JSON value into its Python equivalent.
pub(crate) fn value_to_python(py: Python<'_>, value: serde_json::Value) -> PyResult<Py<PyAny>> {
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

/// Serialize a response view through JSON into its Python equivalent.
pub(crate) fn to_python(py: Python<'_>, value: impl serde::Serialize) -> PyResult<Py<PyAny>> {
    let json = serde_json::to_value(value)
        .map_err(|error| NativeInternalError::new_err(format!("serialization: {error}")))?;
    value_to_python(py, json)
}

/// Borrow the live embedded service or raise a lifecycle error.
pub(crate) fn require(
    inner: &Option<eggchaos_embed::EmbeddedService>,
) -> PyResult<&eggchaos_embed::EmbeddedService> {
    inner
        .as_ref()
        .filter(|service| !service.is_closed())
        .ok_or_else(|| convert_error(EmbedError::Lifecycle("service is closed".into())))
}

/// Convert a Python document into its JSON equivalent for DTO parsing.
pub(crate) fn python_to_value(py: Python<'_>, object: &Py<PyAny>) -> PyResult<serde_json::Value> {
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
