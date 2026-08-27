//! The C++ boundary: serialise, call, deserialise.

use crate::proto;
use prost::Message;

unsafe extern "C" {
    fn cpsat_solve(
        model_buf: *const u8,
        model_len: usize,
        params_buf: *const u8,
        params_len: usize,
        out_len: *mut usize,
    ) -> *mut u8;
    fn cpsat_model_stats(model_buf: *const u8, model_len: usize) -> *mut std::ffi::c_char;
    fn cpsat_validate(model_buf: *const u8, model_len: usize) -> *mut std::ffi::c_char;
    fn cpsat_free(buf: *mut u8);
    fn cpsat_free_string(s: *mut std::ffi::c_char);
}

/// Take ownership of a malloc'd buffer from the shim and copy it into a `Vec`.
unsafe fn take_bytes(ptr: *mut u8, len: usize) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return (len == 0).then(Vec::new);
    }
    let out = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    unsafe { cpsat_free(ptr) };
    Some(out)
}

unsafe fn take_string(ptr: *mut std::ffi::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let out = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe { cpsat_free_string(ptr) };
    Some(out)
}

fn call_solve(
    model: &proto::CpModelProto,
    params: Option<&proto::SatParameters>,
) -> proto::CpSolverResponse {
    let model_buf = model.encode_to_vec();
    let params_buf = params.map(|p| p.encode_to_vec());
    let (params_ptr, params_len) = match &params_buf {
        Some(b) => (b.as_ptr(), b.len()),
        None => (std::ptr::null(), 0),
    };

    let mut out_len = 0usize;
    let bytes = unsafe {
        let ptr = cpsat_solve(
            model_buf.as_ptr(),
            model_buf.len(),
            params_ptr,
            params_len,
            &mut out_len,
        );
        take_bytes(ptr, out_len)
    }
    .expect("CP-SAT rejected a model this crate produced — please file a bug");

    proto::CpSolverResponse::decode(&bytes[..]).expect("malformed CpSolverResponse")
}

/// Solve a model with default parameters.
pub fn solve(model: &proto::CpModelProto) -> proto::CpSolverResponse {
    call_solve(model, None)
}

/// Solve a model with explicit solver parameters.
pub fn solve_with_parameters(
    model: &proto::CpModelProto,
    params: &proto::SatParameters,
) -> proto::CpSolverResponse {
    call_solve(model, Some(params))
}

/// Human-readable summary of a model's size and structure.
pub fn model_stats(model: &proto::CpModelProto) -> String {
    let buf = model.encode_to_vec();
    unsafe { take_string(cpsat_model_stats(buf.as_ptr(), buf.len())) }.unwrap_or_default()
}

/// `Ok(())` if the model is well formed, otherwise CP-SAT's explanation.
pub fn validate(model: &proto::CpModelProto) -> Result<(), String> {
    let buf = model.encode_to_vec();
    match unsafe { take_string(cpsat_validate(buf.as_ptr(), buf.len())) } {
        Some(e) if e.is_empty() => Ok(()),
        Some(e) => Err(e),
        None => Err("model failed to parse".into()),
    }
}
