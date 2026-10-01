//! PyO3 native embedding pilot: managed `Service` over `eggchaos-embed`.
//!
//! # FFI safety note
//!
//! The workspace forbids handwritten `unsafe` in all normal Rust crates,
//! and this binding crate holds the same bar for handwritten code: the
//! crate root denies `unsafe_code`, the PyO3 macro-facing layer
//! (`bridge`: `#[pyclass]`/`#[pymethods]`/`create_exception!`
//! expansions, which are framework-owned FFI glue containing `unsafe`)
//! carries a module-scoped allowance, and every ordinary implementation
//! module (`convert`: pure conversion, validation, and lifecycle
//! helpers) denies `unsafe_code` so the compiler enforces it as
//! unsafe-free. There is no handwritten `unsafe` anywhere in this crate
//! (audited by `check_python_native.sh`: the handwritten-unsafe grep
//! must be empty, no crate-level `allow(unsafe_code)` may exist, and
//! the safe modules must keep their `deny(unsafe_code)`). Any future
//! handwritten `unsafe` stops the milestone and requires a new ADR.
#![deny(unsafe_code)]

use pyo3::prelude::*;

mod bridge;
mod convert;

use bridge::{
    Fault, NativeBindError, NativeConflictError, NativeError, NativeInternalError,
    NativeLifecycleError, NativeNotFoundError, NativeUnsupportedError, NativeValidationError,
    Service,
};

/// Native in-process eggchaos service for Python test harnesses.
// M059: `#[pymodule]` expands to framework-owned FFI glue containing
// `unsafe`; this item-scoped allowance covers exactly that generated
// code while the crate root keeps denying handwritten `unsafe`.
#[allow(unsafe_code)]
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
