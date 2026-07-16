#![cfg_attr(target_arch = "wasm32", no_std)]
#![cfg_attr(
    not(any(target_arch = "wasm32", test)),
    allow(dead_code, unused_imports)
)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::{boxed::Box, format, string::String, vec::Vec};
use core::cell::RefCell;

use mecojoni_core::{
    CompiledGrammar, DataBinding, Diagnostic, DiverseGenerationRequest, GeneratedContent,
    GenerationLimits, GenerationRequest, LocaleRequest, MecoError, MecoResult, MessageArgument,
    MessageDefinition, MessageManifest, PackageInput, PackageSource, Rational, RepetitionSnapshot,
    RepetitionStore, ResolvedImport, SamplerSession, SchemaType, SessionSnapshot, Severity,
    SourceFile, SourceId, Value, compile_package, compile_package_with_manifest,
};

mod wire;

use wire::{Decoder, Encoder, WIRE_VERSION, WireError};

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

/// Version of the handwritten linear-memory ABI.
pub const ABI_VERSION: u32 = 1;
pub const OP_PACKAGE_CREATE: u32 = 1;
pub const OP_COMPILE: u32 = 2;
pub const OP_GENERATE_WEIGHTED: u32 = 3;
pub const OP_GENERATE_TYPED: u32 = 4;
pub const OP_COMPILE_WITH_MANIFEST: u32 = 5;
pub const OP_GENERATE_STRUCTURAL: u32 = 6;
pub const OP_REPETITION_CREATE: u32 = 7;
pub const OP_SESSION_CREATE: u32 = 8;
pub const OP_GENERATE_DIVERSE: u32 = 9;
pub const OP_SESSION_SNAPSHOT_EXPORT: u32 = 10;
pub const OP_SESSION_SNAPSHOT_IMPORT: u32 = 11;
pub const OP_REPETITION_SNAPSHOT_EXPORT: u32 = 12;
pub const OP_REPETITION_SNAPSHOT_IMPORT: u32 = 13;

pub const STATUS_SUCCESS: u32 = 0;
pub const STATUS_ERROR: u32 = 1;
pub const STATUS_INVALID_HANDLE: u32 = 2;

const PAYLOAD_ERROR: u32 = 0;
const PAYLOAD_PACKAGE: u32 = 1;
const PAYLOAD_COMPILE: u32 = 2;
const PAYLOAD_GENERATE: u32 = 3;
const PAYLOAD_STRUCTURAL: u32 = 4;
const PAYLOAD_DIVERSE: u32 = 5;
const PAYLOAD_SNAPSHOT: u32 = 6;

const MAX_MODULES: usize = 4_096;
const MAX_IMPORTS_PER_MODULE: usize = 4_096;
const MAX_STRING_BYTES: usize = 1_048_576;
const MAX_SOURCE_BYTES: usize = 16_777_216;
const MAX_REQUEST_VALUES: usize = 4_096;
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

/// Returns the ABI version before any allocation or handle operation is attempted.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn meco_abi_version() -> u32 {
    ABI_VERSION
}

/// Returns the core Rust API version linked into this adapter.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn meco_core_api_version() -> u32 {
    mecojoni_core::API_VERSION
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HandleKind {
    Package,
    Grammar,
    Result,
    Repetition,
    Session,
}

enum HandleValue {
    Package(Box<PackageInput>),
    Grammar(Box<CompiledGrammar>),
    Result(ResultRecord),
    Repetition(RefCell<RepetitionStore>),
    Session(RefCell<SamplerSession>),
}

impl HandleValue {
    const fn kind(&self) -> HandleKind {
        match self {
            Self::Package(_) => HandleKind::Package,
            Self::Grammar(_) => HandleKind::Grammar,
            Self::Result(_) => HandleKind::Result,
            Self::Repetition(_) => HandleKind::Repetition,
            Self::Session(_) => HandleKind::Session,
        }
    }
}

struct HandleSlot {
    id: u32,
    value: HandleValue,
}

struct ResultRecord {
    status: u32,
    value_handle: u32,
    value_claimed: bool,
    payload: Vec<u8>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
struct ExternalAllocation {
    pointer: u32,
    length: u32,
    alignment: u32,
}

struct State {
    next_handle: u32,
    handles: Vec<HandleSlot>,
    #[cfg(target_arch = "wasm32")]
    allocations: Vec<ExternalAllocation>,
}

impl State {
    const fn new() -> Self {
        Self {
            next_handle: 1,
            handles: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            allocations: Vec::new(),
        }
    }

    fn insert(&mut self, value: HandleValue) -> Option<u32> {
        let id = self.next_handle;
        if id == 0 {
            return None;
        }
        self.next_handle = id.checked_add(1).unwrap_or(0);
        self.handles.push(HandleSlot { id, value });
        Some(id)
    }

    fn get(&self, id: u32) -> Option<&HandleValue> {
        self.handles
            .iter()
            .find(|slot| slot.id == id)
            .map(|slot| &slot.value)
    }

    fn remove(&mut self, id: u32) -> Option<HandleValue> {
        let index = self.handles.iter().position(|slot| slot.id == id)?;
        Some(self.handles.swap_remove(index).value)
    }

    fn dispose(&mut self, id: u32) -> bool {
        let Some(value) = self.remove(id) else {
            return false;
        };
        if let HandleValue::Result(result) = value {
            if result.value_handle != 0 && !result.value_claimed {
                let _ = self.remove(result.value_handle);
            }
        }
        true
    }

    fn claim_result_value(&mut self, handle: u32) -> Option<u32> {
        let slot = self.handles.iter_mut().find(|slot| slot.id == handle)?;
        let HandleValue::Result(result) = &mut slot.value else {
            return None;
        };
        result.value_claimed = true;
        Some(result.value_handle)
    }

    fn add_result(&mut self, record: ResultRecord) -> u32 {
        self.insert(HandleValue::Result(record)).unwrap_or(0)
    }

    fn add_value_result(&mut self, value: HandleValue, payload: Vec<u8>) -> u32 {
        let Some(value_handle) = self.insert(value) else {
            return 0;
        };
        let result = self.add_result(ResultRecord {
            status: STATUS_SUCCESS,
            value_handle,
            value_claimed: false,
            payload,
        });
        if result == 0 {
            let _ = self.remove(value_handle);
        }
        result
    }

    fn add_error(&mut self, diagnostic: AbiDiagnostic) -> u32 {
        let AbiDiagnostic { code, message } = diagnostic;
        self.add_result(ResultRecord {
            status: STATUS_ERROR,
            value_handle: 0,
            value_claimed: false,
            payload: encode_abi_error(code, &message),
        })
    }

    fn result(&self, handle: u32) -> Option<&ResultRecord> {
        match self.get(handle)? {
            HandleValue::Result(result) => Some(result),
            HandleValue::Package(_)
            | HandleValue::Grammar(_)
            | HandleValue::Repetition(_)
            | HandleValue::Session(_) => None,
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn allocation_contains(&self, pointer: u32, length: u32) -> bool {
        let Some(end) = pointer.checked_add(length) else {
            return false;
        };
        self.allocations.iter().any(|allocation| {
            let allocation_end = allocation.pointer.saturating_add(allocation.length);
            allocation.pointer <= pointer && end <= allocation_end
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AbiDiagnostic {
    code: &'static str,
    message: String,
}

impl AbiDiagnostic {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

fn dispatch(state: &mut State, operation: u32, input: &[u8]) -> u32 {
    match operation {
        OP_PACKAGE_CREATE => package_create(state, input),
        OP_COMPILE => compile(state, input),
        OP_GENERATE_WEIGHTED => generate_weighted(state, input, false),
        OP_GENERATE_TYPED => generate_weighted(state, input, true),
        OP_COMPILE_WITH_MANIFEST => compile_with_manifest(state, input),
        OP_GENERATE_STRUCTURAL => generate_structural(state, input),
        OP_REPETITION_CREATE => repetition_create(state, input),
        OP_SESSION_CREATE => session_create(state, input),
        OP_GENERATE_DIVERSE => generate_diverse(state, input),
        OP_SESSION_SNAPSHOT_EXPORT => session_snapshot_export(state, input),
        OP_SESSION_SNAPSHOT_IMPORT => session_snapshot_import(state, input),
        OP_REPETITION_SNAPSHOT_EXPORT => repetition_snapshot_export(state, input),
        OP_REPETITION_SNAPSHOT_IMPORT => repetition_snapshot_import(state, input),
        _ => state.add_error(AbiDiagnostic::new(
            "E_ABI_OPERATION",
            format!("unknown ABI operation {operation}"),
        )),
    }
}

fn package_create(state: &mut State, input: &[u8]) -> u32 {
    match decode_package(input) {
        Ok(package) => {
            let payload = encode_empty_success(PAYLOAD_PACKAGE);
            state.add_value_result(HandleValue::Package(Box::new(package)), payload)
        }
        Err(diagnostic) => state.add_error(diagnostic),
    }
}

fn compile(state: &mut State, input: &[u8]) -> u32 {
    let package_handle = match decode_handle_request(input) {
        Ok(handle) => handle,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let grammar = match state.get(package_handle) {
        Some(HandleValue::Package(package)) => match compile_package(package) {
            Ok(grammar) => grammar,
            Err(error) => return add_core_error(state, &error),
        },
        Some(value) => {
            return state.add_error(wrong_kind(
                package_handle,
                HandleKind::Package,
                value.kind(),
            ));
        }
        None => return state.add_error(stale_handle(package_handle)),
    };
    let payload = encode_compile_success(&grammar);
    state.add_value_result(HandleValue::Grammar(Box::new(grammar)), payload)
}

fn compile_with_manifest(state: &mut State, input: &[u8]) -> u32 {
    let (package_handle, manifest) = match decode_compile_manifest_request(input) {
        Ok(request) => request,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let grammar = match state.get(package_handle) {
        Some(HandleValue::Package(package)) => {
            match compile_package_with_manifest(package, &manifest) {
                Ok(grammar) => grammar,
                Err(error) => return add_core_error(state, &error),
            }
        }
        Some(value) => {
            return state.add_error(wrong_kind(
                package_handle,
                HandleKind::Package,
                value.kind(),
            ));
        }
        None => return state.add_error(stale_handle(package_handle)),
    };
    let payload = encode_compile_success(&grammar);
    state.add_value_result(HandleValue::Grammar(Box::new(grammar)), payload)
}

fn generate_weighted(state: &mut State, input: &[u8], typed: bool) -> u32 {
    let request = match decode_generation_request(input, typed, false, false) {
        Ok(request) => request,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let result = match state.get(request.grammar) {
        Some(HandleValue::Grammar(grammar)) => {
            let entry = request.entry.as_deref();
            grammar.generate_weighted(&GenerationRequest {
                entry,
                seed: request.seed,
                limits: request.limits,
                data: &request.data,
                trace_bindings: request.trace_bindings,
                trace_selections: request.trace_selections,
                trace_provenance: request.trace_provenance,
            })
        }
        Some(value) => {
            return state.add_error(wrong_kind(
                request.grammar,
                HandleKind::Grammar,
                value.kind(),
            ));
        }
        None => return state.add_error(stale_handle(request.grammar)),
    };
    match result {
        Ok(result) => state.add_result(ResultRecord {
            status: STATUS_SUCCESS,
            value_handle: 0,
            value_claimed: false,
            payload: encode_generation_success(&result, typed),
        }),
        Err(error) => add_core_error(state, &error),
    }
}

fn generate_structural(state: &mut State, input: &[u8]) -> u32 {
    let request = match decode_generation_request(input, true, true, false) {
        Ok(request) => request,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let result = match state.get(request.grammar) {
        Some(HandleValue::Grammar(grammar)) => grammar.generate_weighted_structural(
            &GenerationRequest {
                entry: request.entry.as_deref(),
                seed: request.seed,
                limits: request.limits,
                data: &request.data,
                trace_bindings: request.trace_bindings,
                trace_selections: request.trace_selections,
                trace_provenance: request.trace_provenance,
            },
            Some(LocaleRequest {
                requested: request
                    .requested_locale
                    .as_deref()
                    .expect("localized request decoder sets a locale"),
                fallbacks: &request
                    .fallback_locales
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            }),
        ),
        Some(value) => {
            return state.add_error(wrong_kind(
                request.grammar,
                HandleKind::Grammar,
                value.kind(),
            ));
        }
        None => return state.add_error(stale_handle(request.grammar)),
    };
    match result {
        Ok(result) => state.add_result(ResultRecord {
            status: STATUS_SUCCESS,
            value_handle: 0,
            value_claimed: false,
            payload: encode_structural_success(&result),
        }),
        Err(error) => add_core_error(state, &error),
    }
}

fn repetition_create(state: &mut State, input: &[u8]) -> u32 {
    let decoder = match decoder(input) {
        Ok(decoder) => decoder,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    if let Err(error) = decoder.finish() {
        return state.add_error(wire_diagnostic(error));
    }
    state.add_value_result(
        HandleValue::Repetition(RefCell::new(RepetitionStore::new_location())),
        encode_empty_success(PAYLOAD_PACKAGE),
    )
}

fn session_create(state: &mut State, input: &[u8]) -> u32 {
    let mut decoder = match decoder(input) {
        Ok(decoder) => decoder,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let seed = match decoder.u64() {
        Ok(seed) => seed,
        Err(error) => return state.add_error(wire_diagnostic(error)),
    };
    if let Err(error) = decoder.finish() {
        return state.add_error(wire_diagnostic(error));
    }
    state.add_value_result(
        HandleValue::Session(RefCell::new(SamplerSession::new(seed))),
        encode_empty_success(PAYLOAD_PACKAGE),
    )
}

fn session_snapshot_export(state: &mut State, input: &[u8]) -> u32 {
    let handle = match decode_handle_request(input) {
        Ok(handle) => handle,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let result = match state.get(handle) {
        Some(HandleValue::Session(session)) => session
            .try_borrow()
            .map(|session| session.snapshot().to_bytes())
            .map_err(|_| {
                AbiDiagnostic::new(
                    "E_STATE_BUSY",
                    "sampler session already has an active operation",
                )
            }),
        Some(value) => Err(wrong_kind(handle, HandleKind::Session, value.kind())),
        None => Err(stale_handle(handle)),
    };
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => return state.add_error(error),
    };
    state.add_result(ResultRecord {
        status: STATUS_SUCCESS,
        value_handle: 0,
        value_claimed: false,
        payload: encode_snapshot_success(&bytes),
    })
}

fn session_snapshot_import(state: &mut State, input: &[u8]) -> u32 {
    let bytes = match decode_snapshot_bytes(input) {
        Ok(bytes) => bytes,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let snapshot = match SessionSnapshot::from_bytes(&bytes) {
        Ok(snapshot) => snapshot,
        Err(error) => return add_core_error(state, &error),
    };
    let session = match SamplerSession::restore(snapshot) {
        Ok(session) => session,
        Err(error) => return add_core_error(state, &error),
    };
    state.add_value_result(
        HandleValue::Session(RefCell::new(session)),
        encode_snapshot_success(&bytes),
    )
}

fn repetition_snapshot_export(state: &mut State, input: &[u8]) -> u32 {
    let handle = match decode_handle_request(input) {
        Ok(handle) => handle,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let result = match state.get(handle) {
        Some(HandleValue::Repetition(store)) => match store.try_borrow() {
            Ok(store) => store.snapshot().map(|snapshot| snapshot.to_bytes()),
            Err(_) => Err(MecoError::new(Diagnostic::new(
                mecojoni_core::DiagnosticCode::STATE_BUSY,
                Severity::Error,
                None,
                "repetition store already has an active operation",
            ))),
        },
        Some(value) => {
            return state.add_error(wrong_kind(handle, HandleKind::Repetition, value.kind()));
        }
        None => return state.add_error(stale_handle(handle)),
    };
    match result {
        Ok(bytes) => state.add_result(ResultRecord {
            status: STATUS_SUCCESS,
            value_handle: 0,
            value_claimed: false,
            payload: encode_snapshot_success(&bytes),
        }),
        Err(error) => add_core_error(state, &error),
    }
}

fn repetition_snapshot_import(state: &mut State, input: &[u8]) -> u32 {
    let bytes = match decode_snapshot_bytes(input) {
        Ok(bytes) => bytes,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let snapshot = match RepetitionSnapshot::from_bytes(&bytes) {
        Ok(snapshot) => snapshot,
        Err(error) => return add_core_error(state, &error),
    };
    let store = match RepetitionStore::restore(&snapshot) {
        Ok(store) => store,
        Err(error) => return add_core_error(state, &error),
    };
    state.add_value_result(
        HandleValue::Repetition(RefCell::new(store)),
        encode_snapshot_success(&bytes),
    )
}

fn decode_snapshot_bytes(input: &[u8]) -> Result<Vec<u8>, AbiDiagnostic> {
    let mut decoder = decoder(input)?;
    let bytes = decoder.bytes(MAX_SNAPSHOT_BYTES).map_err(wire_diagnostic)?;
    decoder.finish().map_err(wire_diagnostic)?;
    Ok(bytes)
}

fn generate_diverse(state: &mut State, input: &[u8]) -> u32 {
    let request = match decode_generation_request(input, true, false, true) {
        Ok(request) => request,
        Err(diagnostic) => return state.add_error(diagnostic),
    };
    let grammar = match state.get(request.grammar) {
        Some(HandleValue::Grammar(grammar)) => grammar,
        Some(value) => {
            return state.add_error(wrong_kind(
                request.grammar,
                HandleKind::Grammar,
                value.kind(),
            ));
        }
        None => return state.add_error(stale_handle(request.grammar)),
    };
    let session_handle = request.session.expect("stateful decoder sets session");
    let store_handle = request.repetition.expect("stateful decoder sets store");
    let session = match state.get(session_handle) {
        Some(HandleValue::Session(session)) => session,
        Some(value) => {
            return state.add_error(wrong_kind(
                session_handle,
                HandleKind::Session,
                value.kind(),
            ));
        }
        None => return state.add_error(stale_handle(session_handle)),
    };
    let store = match state.get(store_handle) {
        Some(HandleValue::Repetition(store)) => store,
        Some(value) => {
            return state.add_error(wrong_kind(
                store_handle,
                HandleKind::Repetition,
                value.kind(),
            ));
        }
        None => return state.add_error(stale_handle(store_handle)),
    };
    let result = (|| -> MecoResult<Vec<u8>> {
        let mut session = session.try_borrow_mut().map_err(|_| {
            MecoError::new(Diagnostic::new(
                mecojoni_core::DiagnosticCode::STATE_BUSY,
                Severity::Error,
                None,
                "sampler session already has an active operation",
            ))
        })?;
        let mut store = store.try_borrow_mut().map_err(|_| {
            MecoError::new(Diagnostic::new(
                mecojoni_core::DiagnosticCode::STATE_BUSY,
                Severity::Error,
                None,
                "repetition store already has an active operation",
            ))
        })?;
        session
            .generate(
                grammar,
                &mut store,
                &DiverseGenerationRequest {
                    entry: request.entry.as_deref(),
                    limits: request.limits,
                    data: &request.data,
                    trace_bindings: request.trace_bindings,
                    trace_selections: request.trace_selections,
                    trace_provenance: request.trace_provenance,
                    cancelled: request.cancelled,
                },
            )
            .map(|result| encode_diverse_success(&result))
    })();
    match result {
        Ok(payload) => state.add_result(ResultRecord {
            status: STATUS_SUCCESS,
            value_handle: 0,
            value_claimed: false,
            payload,
        }),
        Err(error) => add_core_error(state, &error),
    }
}

#[allow(clippy::struct_excessive_bools)]
struct AbiGenerationRequest {
    grammar: u32,
    seed: u64,
    entry: Option<String>,
    limits: GenerationLimits,
    data: Vec<DataBinding>,
    trace_bindings: bool,
    trace_selections: bool,
    trace_provenance: bool,
    requested_locale: Option<String>,
    fallback_locales: Vec<String>,
    session: Option<u32>,
    repetition: Option<u32>,
    cancelled: bool,
}

fn decode_handle_request(input: &[u8]) -> Result<u32, AbiDiagnostic> {
    let mut decoder = decoder(input)?;
    let handle = decoder.u32().map_err(wire_diagnostic)?;
    decoder.finish().map_err(wire_diagnostic)?;
    if handle == 0 {
        return Err(AbiDiagnostic::new(
            "E_ABI_HANDLE_STALE",
            "handle 0 is always invalid",
        ));
    }
    Ok(handle)
}

#[allow(clippy::too_many_lines)]
fn decode_generation_request(
    input: &[u8],
    typed: bool,
    localized: bool,
    stateful: bool,
) -> Result<AbiGenerationRequest, AbiDiagnostic> {
    let mut decoder = decoder(input)?;
    let grammar = decoder.u32().map_err(wire_diagnostic)?;
    let seed = decoder.u64().map_err(wire_diagnostic)?;
    if stateful && seed != 0 {
        return Err(AbiDiagnostic::new(
            "E_ABI_WIRE_VALUE",
            "stateful diverse request reserved seed must be zero",
        ));
    }
    let entry = decoder
        .optional_string(MAX_STRING_BYTES)
        .map_err(wire_diagnostic)?;
    let limits = GenerationLimits {
        max_depth: decoder.u32().map_err(wire_diagnostic)?,
        max_expansions: decoder.u32().map_err(wire_diagnostic)?,
        max_output_scalars: decoder.u32().map_err(wire_diagnostic)?,
        max_output_bytes: decoder.u32().map_err(wire_diagnostic)?,
        max_sampler_words: decoder.u32().map_err(wire_diagnostic)?,
    };
    let (trace_bindings, trace_selections, trace_provenance, data) = if typed {
        let trace_bindings = match decoder.u8().map_err(wire_diagnostic)? {
            0 => false,
            1 => true,
            _ => {
                return Err(AbiDiagnostic::new(
                    "E_ABI_WIRE_VALUE",
                    "binding trace flag must be 0 or 1",
                ));
            }
        };
        let trace_selections = match decoder.u8().map_err(wire_diagnostic)? {
            0 => false,
            1 => true,
            _ => {
                return Err(AbiDiagnostic::new(
                    "E_ABI_WIRE_VALUE",
                    "selection trace flag must be 0 or 1",
                ));
            }
        };
        let trace_provenance = match decoder.u8().map_err(wire_diagnostic)? {
            0 => false,
            1 => true,
            _ => {
                return Err(AbiDiagnostic::new(
                    "E_ABI_WIRE_VALUE",
                    "provenance trace flag must be 0 or 1",
                ));
            }
        };
        let value_count = count(&mut decoder, MAX_REQUEST_VALUES, "request value")?;
        let mut data = Vec::with_capacity(value_count);
        for _ in 0..value_count {
            let name = decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?;
            let value = decode_value(&mut decoder)?;
            data.push(DataBinding::new(name, value));
        }
        (trace_bindings, trace_selections, trace_provenance, data)
    } else {
        (false, false, false, Vec::new())
    };
    let (requested_locale, fallback_locales) = if localized {
        let requested = decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?;
        let count = count(&mut decoder, 256, "fallback locale")?;
        let mut fallbacks = Vec::with_capacity(count);
        for _ in 0..count {
            fallbacks.push(decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?);
        }
        (Some(requested), fallbacks)
    } else {
        (None, Vec::new())
    };
    let (session, repetition, cancelled) = if stateful {
        let state = (
            Some(decoder.u32().map_err(wire_diagnostic)?),
            Some(decoder.u32().map_err(wire_diagnostic)?),
        );
        let cancelled = match decoder.u8().map_err(wire_diagnostic)? {
            0 => false,
            1 => true,
            _ => {
                return Err(AbiDiagnostic::new(
                    "E_ABI_WIRE_VALUE",
                    "cancellation flag must be 0 or 1",
                ));
            }
        };
        (state.0, state.1, cancelled)
    } else {
        (None, None, false)
    };
    decoder.finish().map_err(wire_diagnostic)?;
    if grammar == 0 {
        return Err(AbiDiagnostic::new(
            "E_ABI_HANDLE_STALE",
            "handle 0 is always invalid",
        ));
    }
    Ok(AbiGenerationRequest {
        grammar,
        seed,
        entry,
        limits,
        data,
        trace_bindings,
        trace_selections,
        trace_provenance,
        requested_locale,
        fallback_locales,
        session,
        repetition,
        cancelled,
    })
}

fn decode_compile_manifest_request(input: &[u8]) -> Result<(u32, MessageManifest), AbiDiagnostic> {
    let mut decoder = decoder(input)?;
    let handle = decoder.u32().map_err(wire_diagnostic)?;
    if handle == 0 {
        return Err(AbiDiagnostic::new(
            "E_ABI_HANDLE_STALE",
            "handle 0 is always invalid",
        ));
    }
    let message_count = count(&mut decoder, 65_536, "manifest message")?;
    let mut messages = Vec::with_capacity(message_count);
    for _ in 0..message_count {
        let id = decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?;
        let argument_count = count(&mut decoder, 4_096, "message argument")?;
        let mut arguments = Vec::with_capacity(argument_count);
        for _ in 0..argument_count {
            let name = decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?;
            let type_ = match decoder.u8().map_err(wire_diagnostic)? {
                0 => SchemaType::Text,
                1 => SchemaType::Number,
                2 => SchemaType::Boolean,
                3 => SchemaType::Enum(decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?),
                _ => {
                    return Err(AbiDiagnostic::new(
                        "E_ABI_WIRE_VALUE",
                        "unknown manifest schema type",
                    ));
                }
            };
            arguments.push(MessageArgument { name, type_ });
        }
        messages.push(MessageDefinition { id, arguments });
    }
    decoder.finish().map_err(wire_diagnostic)?;
    Ok((handle, MessageManifest { messages }))
}

fn decode_value(decoder: &mut Decoder<'_>) -> Result<Value, AbiDiagnostic> {
    match decoder.u8().map_err(wire_diagnostic)? {
        0 => decoder
            .string(MAX_STRING_BYTES)
            .map(Value::Text)
            .map_err(wire_diagnostic),
        1 => {
            let numerator =
                i64::from_le_bytes(decoder.u64().map_err(wire_diagnostic)?.to_le_bytes());
            let denominator = decoder.u64().map_err(wire_diagnostic)?;
            Rational::new(numerator, denominator)
                .map(Value::Number)
                .map_err(|error| {
                    AbiDiagnostic::new(
                        "E_ABI_WIRE_VALUE",
                        format!("invalid rational request value: {error}"),
                    )
                })
        }
        2 => match decoder.u8().map_err(wire_diagnostic)? {
            0 => Ok(Value::Boolean(false)),
            1 => Ok(Value::Boolean(true)),
            _ => Err(AbiDiagnostic::new(
                "E_ABI_WIRE_VALUE",
                "boolean request value must be 0 or 1",
            )),
        },
        3 => decoder
            .string(MAX_STRING_BYTES)
            .map(Value::Enum)
            .map_err(wire_diagnostic),
        _ => Err(AbiDiagnostic::new(
            "E_ABI_WIRE_VALUE",
            "unknown request value kind",
        )),
    }
}

fn decode_package(input: &[u8]) -> Result<PackageInput, AbiDiagnostic> {
    let mut decoder = decoder(input)?;
    let root_id = decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?;
    let module_count = count(&mut decoder, MAX_MODULES, "module")?;
    if module_count == 0 {
        return Err(AbiDiagnostic::new(
            "E_ABI_WIRE_VALUE",
            "a package request requires at least one module",
        ));
    }
    let mut modules = Vec::with_capacity(module_count);
    let mut source_ids = Vec::with_capacity(module_count);
    for _ in 0..module_count {
        let canonical_id = decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?;
        let source_id = decoder.u32().map_err(wire_diagnostic)?;
        if source_ids.contains(&source_id) {
            return Err(AbiDiagnostic::new(
                "E_ABI_WIRE_VALUE",
                format!("duplicate source ID {source_id}"),
            ));
        }
        source_ids.push(source_id);
        let source_name = decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?;
        let source_bytes = decoder.bytes(MAX_SOURCE_BYTES).map_err(wire_diagnostic)?;
        let source = SourceFile::from_utf8(SourceId::new(source_id), source_name, &source_bytes)
            .map_err(|error| {
                AbiDiagnostic::new("E_INVALID_UTF8", format!("source {source_id}: {error}"))
            })?;
        let import_count = count(&mut decoder, MAX_IMPORTS_PER_MODULE, "import")?;
        let mut resolved_imports = Vec::with_capacity(import_count);
        for _ in 0..import_count {
            resolved_imports.push(ResolvedImport {
                authored_path: decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?,
                target_id: decoder.string(MAX_STRING_BYTES).map_err(wire_diagnostic)?,
            });
        }
        modules.push(PackageSource {
            canonical_id,
            source,
            resolved_imports,
        });
    }
    decoder.finish().map_err(wire_diagnostic)?;
    Ok(PackageInput { root_id, modules })
}

fn decoder(input: &[u8]) -> Result<Decoder<'_>, AbiDiagnostic> {
    let mut decoder = Decoder::new(input);
    let version = decoder.u32().map_err(wire_diagnostic)?;
    if version != WIRE_VERSION {
        return Err(AbiDiagnostic::new(
            "E_ABI_WIRE_VERSION",
            format!("wire version {version} is unsupported"),
        ));
    }
    Ok(decoder)
}

fn count(decoder: &mut Decoder<'_>, maximum: usize, label: &str) -> Result<usize, AbiDiagnostic> {
    let value = usize::try_from(decoder.u32().map_err(wire_diagnostic)?)
        .map_err(|_| AbiDiagnostic::new("E_ABI_WIRE_LIMIT", "count does not fit this target"))?;
    if value > maximum {
        return Err(AbiDiagnostic::new(
            "E_ABI_WIRE_LIMIT",
            format!("{label} count exceeds {maximum}"),
        ));
    }
    Ok(value)
}

fn wire_diagnostic(error: WireError) -> AbiDiagnostic {
    let (code, message) = match error {
        WireError::Truncated => ("E_ABI_WIRE_TRUNCATED", "wire payload is truncated"),
        WireError::Limit => ("E_ABI_WIRE_LIMIT", "wire field exceeds its size limit"),
        WireError::InvalidUtf8 => ("E_ABI_UTF8", "wire string is not strict UTF-8"),
        WireError::TrailingBytes => ("E_ABI_WIRE_TRAILING", "wire payload has trailing bytes"),
        WireError::InvalidValue => ("E_ABI_WIRE_VALUE", "wire field has an invalid value"),
    };
    AbiDiagnostic::new(code, message)
}

fn stale_handle(handle: u32) -> AbiDiagnostic {
    AbiDiagnostic::new(
        "E_ABI_HANDLE_STALE",
        format!("handle {handle} is unknown or disposed"),
    )
}

fn wrong_kind(handle: u32, expected: HandleKind, actual: HandleKind) -> AbiDiagnostic {
    AbiDiagnostic::new(
        "E_ABI_HANDLE_KIND",
        format!("handle {handle} is {actual:?}, expected {expected:?}"),
    )
}

fn add_core_error(state: &mut State, error: &MecoError) -> u32 {
    state.add_result(ResultRecord {
        status: STATUS_ERROR,
        value_handle: 0,
        value_claimed: false,
        payload: encode_core_error(error),
    })
}

fn encode_empty_success(kind: u32) -> Vec<u8> {
    let encoder = payload(kind);
    encoder.into_bytes()
}

fn encode_snapshot_success(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = payload(PAYLOAD_SNAPSHOT);
    encoder.bytes(bytes);
    encoder.into_bytes()
}

fn encode_compile_success(grammar: &CompiledGrammar) -> Vec<u8> {
    let mut encoder = payload(PAYLOAD_COMPILE);
    encoder.u32(u32::try_from(grammar.entries().len()).unwrap_or(u32::MAX));
    for entry in grammar.entries() {
        encoder.string(entry);
    }
    match grammar.default_entry() {
        Some(entry) => {
            encoder.u8(1);
            encoder.string(entry);
        }
        None => encoder.u8(0),
    }
    encode_diagnostics(&mut encoder, grammar.warnings());
    encoder.into_bytes()
}

fn encode_generation_success(result: &mecojoni_core::GenerationResult, typed: bool) -> Vec<u8> {
    let mut encoder = payload(PAYLOAD_GENERATE);
    encoder.string(result.text());
    encoder.string(result.entry());
    encoder.u32(result.expansions());
    encoder.u32(result.sampler_words());
    if typed {
        encode_traces(
            &mut encoder,
            result.bindings(),
            result.selections(),
            result.provenance(),
        );
    }
    encoder.into_bytes()
}

fn encode_structural_success(result: &mecojoni_core::StructuralGenerationResult) -> Vec<u8> {
    let mut encoder = payload(PAYLOAD_STRUCTURAL);
    match result.content() {
        GeneratedContent::Text(text) => {
            encoder.u8(0);
            encoder.string(text);
        }
        GeneratedContent::Message(request) => {
            encoder.u8(1);
            encoder.string(request.message_id());
            encoder.u32(u32::try_from(request.arguments().len()).unwrap_or(u32::MAX));
            for (name, value) in request.arguments() {
                encoder.string(name);
                encode_value(&mut encoder, value);
            }
            encoder.string(request.requested_locale());
            encoder.u32(u32::try_from(request.fallback_locales().len()).unwrap_or(u32::MAX));
            for locale in request.fallback_locales() {
                encoder.string(locale);
            }
        }
    }
    encoder.string(result.entry());
    encoder.u32(result.expansions());
    encoder.u32(result.sampler_words());
    encode_traces(
        &mut encoder,
        result.bindings(),
        result.selections(),
        result.provenance(),
    );
    encoder.into_bytes()
}

fn encode_diverse_success(result: &mecojoni_core::DiverseResult) -> Vec<u8> {
    let generation = result.generation();
    let mut encoder = payload(PAYLOAD_DIVERSE);
    encoder.string(generation.text());
    encoder.string(generation.entry());
    encoder.u32(generation.expansions());
    encoder.u32(generation.sampler_words());
    encode_traces(
        &mut encoder,
        generation.bindings(),
        generation.selections(),
        generation.provenance(),
    );
    encoder.u32(result.attempts());
    encoder.u32(result.winner_attempt());
    encoder.u32(result.exact_repetitions());
    encoder.u64(result.edge_repetitions());
    encoder.u64(result.committed_revision());
    encode_replay_receipt(&mut encoder, result.receipt());
    encoder.into_bytes()
}

fn encode_traces(
    encoder: &mut Encoder,
    bindings: &[mecojoni_core::BindingTrace],
    selections: &[mecojoni_core::SelectionTrace],
    provenance: &[mecojoni_core::ProvenanceNode],
) {
    encoder.u32(u32::try_from(bindings.len()).unwrap_or(u32::MAX));
    for binding in bindings {
        encoder.string(binding.name());
        encoder.u8(u8::from(binding.emitted()));
        encode_value(encoder, binding.value());
    }
    encoder.u32(u32::try_from(selections.len()).unwrap_or(u32::MAX));
    for selection in selections {
        encoder.string(selection.rule());
        encoder.u32(selection.selected_production());
        encoder.string(selection.selected_production_id());
        encoder.u32(u32::try_from(selection.eligible().len()).unwrap_or(u32::MAX));
        for weight in selection.eligible() {
            encoder.u32(weight.production());
            encoder.string(weight.production_id());
            encoder.u64(u64::from_le_bytes(
                weight.base_weight().numerator().to_le_bytes(),
            ));
            encoder.u64(weight.base_weight().denominator());
            encoder.u64(weight.normalized_weight());
        }
    }
    encoder.u32(u32::try_from(provenance.len()).unwrap_or(u32::MAX));
    for node in provenance {
        encoder.u32(node.id());
        match node.parent() {
            Some(parent) => {
                encoder.u8(1);
                encoder.u32(parent);
            }
            None => encoder.u8(0),
        }
        encoder.u8(match node.kind() {
            mecojoni_core::ProvenanceKind::Production => 0,
            mecojoni_core::ProvenanceKind::AuthoredText => 1,
            mecojoni_core::ProvenanceKind::HostValue => 2,
            mecojoni_core::ProvenanceKind::BoundValue => 3,
            mecojoni_core::ProvenanceKind::EmittingCapture => 4,
            mecojoni_core::ProvenanceKind::Binding => 5,
            mecojoni_core::ProvenanceKind::Message => 6,
        });
        encoder.string(node.rule());
        encoder.string(node.production_id());
        encode_span(encoder, node.source_span());
        match node.output() {
            Some(range) => {
                encoder.u8(1);
                encoder.u64(range.start_byte());
                encoder.u64(range.end_byte());
                encoder.u64(range.start_scalar());
                encoder.u64(range.end_scalar());
            }
            None => encoder.u8(0),
        }
        encoder.u32(node.depth());
        match node.name() {
            Some(name) => {
                encoder.u8(1);
                encoder.string(name);
            }
            None => encoder.u8(0),
        }
    }
}

fn encode_replay_receipt(encoder: &mut Encoder, receipt: &mecojoni_core::ReplayReceipt) {
    encoder.u32(receipt.version());
    encoder.u64(receipt.grammar_hash());
    encoder.string(receipt.sampler_version());
    encoder.string(receipt.normalizer_version());
    encoder.string(receipt.tokenizer_version());
    encoder.u64(receipt.pre_session_hash());
    encoder.u64(receipt.pre_session_words());
    encoder.u64(receipt.pre_repetition_hash());
    encoder.u64(receipt.pre_repetition_revision());
    encoder.u64(receipt.reserved_words());
    encoder.u64(receipt.request_digest());
    encoder.string(receipt.effective_entry());
    encoder.u32(receipt.winner_attempt());
    encoder.u64(receipt.derivation_hash());
    encoder.u64(receipt.final_text_hash());
    encoder.u64(receipt.post_session_hash());
    encoder.u64(receipt.post_repetition_hash());
    encoder.u64(receipt.post_repetition_revision());
}

fn encode_span(encoder: &mut Encoder, span: mecojoni_core::Span) {
    encoder.u32(span.source().get());
    encoder.u64(span.start().byte());
    encoder.u64(span.start().scalar());
    encoder.u64(span.end().byte());
    encoder.u64(span.end().scalar());
}

fn encode_value(encoder: &mut Encoder, value: &Value) {
    match value {
        Value::Text(value) => {
            encoder.u8(0);
            encoder.string(value);
        }
        Value::Number(value) => {
            encoder.u8(1);
            encoder.u64(u64::from_le_bytes(value.numerator().to_le_bytes()));
            encoder.u64(value.denominator());
        }
        Value::Boolean(value) => {
            encoder.u8(2);
            encoder.u8(u8::from(*value));
        }
        Value::Enum(value) => {
            encoder.u8(3);
            encoder.string(value);
        }
    }
}

fn encode_abi_error(code: &str, message: &str) -> Vec<u8> {
    let mut encoder = payload(PAYLOAD_ERROR);
    encoder.u32(1);
    encoder.string(code);
    encoder.u8(0);
    encoder.u8(0);
    encoder.string(message);
    encoder.into_bytes()
}

fn encode_core_error(error: &MecoError) -> Vec<u8> {
    let mut encoder = payload(PAYLOAD_ERROR);
    encode_diagnostics(&mut encoder, error.diagnostics());
    encoder.into_bytes()
}

fn encode_diagnostics(encoder: &mut Encoder, diagnostics: &[Diagnostic]) {
    encoder.u32(u32::try_from(diagnostics.len()).unwrap_or(u32::MAX));
    for diagnostic in diagnostics {
        encoder.string(diagnostic.code().as_str());
        encoder.u8(match diagnostic.severity() {
            Severity::Error => 0,
            Severity::Warning => 1,
        });
        if let Some(span) = diagnostic.span() {
            encoder.u8(1);
            encoder.u32(span.source().get());
            encoder.u64(span.start().byte());
            encoder.u64(span.start().scalar());
            encoder.u64(span.end().byte());
            encoder.u64(span.end().scalar());
        } else {
            encoder.u8(0);
        }
        encoder.string(diagnostic.message());
    }
}

fn payload(kind: u32) -> Encoder {
    let mut encoder = Encoder::new();
    encoder.u32(WIRE_VERSION);
    encoder.u32(kind);
    encoder
}

#[cfg(target_arch = "wasm32")]
mod wasm_memory {
    use alloc::alloc::{Layout, alloc, dealloc};
    use core::{cell::UnsafeCell, ptr, slice};

    use super::{ExternalAllocation, State, dispatch};

    struct GlobalState(UnsafeCell<Option<State>>);

    #[allow(unsafe_code)]
    unsafe impl Sync for GlobalState {}

    static STATE: GlobalState = GlobalState(UnsafeCell::new(None));

    #[allow(unsafe_code)]
    fn with_state<R>(callback: impl FnOnce(&mut State) -> R) -> R {
        // `wasm32-unknown-unknown` is instantiated without shared memory or
        // callbacks. Every ABI call is synchronous and non-reentrant.
        let slot = unsafe { &mut *STATE.0.get() };
        callback(slot.get_or_insert_with(State::new))
    }

    fn valid_layout(length: u32, alignment: u32) -> Option<Layout> {
        if length == 0 || alignment == 0 || alignment > 64 || !alignment.is_power_of_two() {
            return None;
        }
        Layout::from_size_align(length as usize, alignment as usize).ok()
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_alloc(length: u32, alignment: u32) -> u32 {
        let Some(layout) = valid_layout(length, alignment) else {
            return 0;
        };
        let pointer = unsafe { alloc(layout) };
        if pointer.is_null() {
            return 0;
        }
        let Ok(pointer) = u32::try_from(pointer as usize) else {
            unsafe { dealloc(pointer, layout) };
            return 0;
        };
        with_state(|state| {
            state.allocations.push(ExternalAllocation {
                pointer,
                length,
                alignment,
            });
        });
        pointer
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_dealloc(pointer: u32, length: u32, alignment: u32) {
        let allocation = with_state(|state| {
            let index = state.allocations.iter().position(|allocation| {
                allocation.pointer == pointer
                    && allocation.length == length
                    && allocation.alignment == alignment
            })?;
            Some(state.allocations.swap_remove(index))
        });
        let (Some(allocation), Some(layout)) = (allocation, valid_layout(length, alignment)) else {
            return;
        };
        unsafe { dealloc(allocation.pointer as *mut u8, layout) };
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_call(operation: u32, input_pointer: u32, input_length: u32) -> u32 {
        let input = with_state(|state| {
            if !state.allocation_contains(input_pointer, input_length) {
                return None;
            }
            let bytes =
                unsafe { slice::from_raw_parts(input_pointer as *const u8, input_length as usize) };
            Some(bytes.to_vec())
        });
        input.map_or(0, |input| {
            with_state(|state| dispatch(state, operation, &input))
        })
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_result_status(result: u32) -> u32 {
        with_state(|state| {
            state
                .result(result)
                .map_or(super::STATUS_INVALID_HANDLE, |record| record.status)
        })
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_result_value_handle(result: u32) -> u32 {
        with_state(|state| state.claim_result_value(result).unwrap_or(0))
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_result_payload_length(result: u32) -> u32 {
        with_state(|state| {
            state.result(result).map_or(0, |record| {
                u32::try_from(record.payload.len()).unwrap_or(u32::MAX)
            })
        })
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_result_payload_copy(
        result: u32,
        destination: u32,
        capacity: u32,
    ) -> u32 {
        with_state(|state| {
            let Some(record) = state.result(result) else {
                return 0;
            };
            let required = u32::try_from(record.payload.len()).unwrap_or(u32::MAX);
            if capacity < required || !state.allocation_contains(destination, capacity) {
                return required;
            }
            if required != 0 {
                unsafe {
                    ptr::copy_nonoverlapping(
                        record.payload.as_ptr(),
                        destination as *mut u8,
                        required as usize,
                    );
                }
            }
            required
        })
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_handle_dispose(handle: u32) {
        with_state(|state| {
            let _ = state.dispose(handle);
        });
    }

    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn meco_live_handle_count() -> u32 {
        with_state(|state| u32::try_from(state.handles.len()).unwrap_or(u32::MAX))
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_memory::{
    meco_alloc, meco_call, meco_dealloc, meco_handle_dispose, meco_live_handle_count,
    meco_result_payload_copy, meco_result_payload_length, meco_result_status,
    meco_result_value_handle,
};

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec};

    use super::{
        ABI_VERSION, HandleKind, HandleValue, MAX_SNAPSHOT_BYTES, OP_COMPILE,
        OP_COMPILE_WITH_MANIFEST, OP_GENERATE_DIVERSE, OP_GENERATE_STRUCTURAL,
        OP_GENERATE_WEIGHTED, OP_PACKAGE_CREATE, OP_REPETITION_CREATE,
        OP_REPETITION_SNAPSHOT_EXPORT, OP_REPETITION_SNAPSHOT_IMPORT, OP_SESSION_CREATE,
        OP_SESSION_SNAPSHOT_EXPORT, OP_SESSION_SNAPSHOT_IMPORT, PAYLOAD_DIVERSE, PAYLOAD_ERROR,
        PAYLOAD_GENERATE, PAYLOAD_SNAPSHOT, PAYLOAD_STRUCTURAL, STATUS_ERROR, STATUS_SUCCESS,
        State, WIRE_VERSION, dispatch, meco_abi_version, meco_core_api_version,
    };
    use crate::wire::{Decoder, Encoder};

    fn package_request() -> Vec<u8> {
        let source = concat!(
            "---\nmeco: 2\nmodule: root\nentry: line\nexports: [line]\n---\n\n",
            "# line\n- hello\n",
        );
        let mut encoder = Encoder::new();
        encoder.u32(WIRE_VERSION);
        encoder.string("root");
        encoder.u32(1);
        encoder.string("root");
        encoder.u32(0);
        encoder.string("root.meco.md");
        encoder.bytes(source.as_bytes());
        encoder.u32(0);
        encoder.into_bytes()
    }

    fn handle_request(handle: u32) -> Vec<u8> {
        let mut encoder = Encoder::new();
        encoder.u32(WIRE_VERSION);
        encoder.u32(handle);
        encoder.into_bytes()
    }

    fn localized_package_request() -> Vec<u8> {
        let source = concat!(
            "---\nmeco: 2\nmodule: root\nentry: arrival\n",
            "inputs:\n  itemCount: number\nexports: [arrival]\n---\n\n",
            "# arrival\n- &arrival <- hero: \"Ada\", count: $itemCount\n",
        );
        let mut encoder = Encoder::new();
        encoder.u32(WIRE_VERSION);
        encoder.string("root");
        encoder.u32(1);
        encoder.string("root");
        encoder.u32(0);
        encoder.string("root.meco.md");
        encoder.bytes(source.as_bytes());
        encoder.u32(0);
        encoder.into_bytes()
    }

    fn compile_manifest_request(handle: u32) -> Vec<u8> {
        let mut encoder = Encoder::new();
        encoder.u32(WIRE_VERSION);
        encoder.u32(handle);
        encoder.u32(1);
        encoder.string("arrival");
        encoder.u32(2);
        encoder.string("hero");
        encoder.u8(0);
        encoder.string("count");
        encoder.u8(1);
        encoder.into_bytes()
    }

    fn structural_request(grammar: u32) -> Vec<u8> {
        let mut encoder = Encoder::new();
        encoder.u32(WIRE_VERSION);
        encoder.u32(grammar);
        encoder.u64(0);
        encoder.u8(0);
        encoder.u32(80);
        encoder.u32(2_000);
        encoder.u32(16_384);
        encoder.u32(65_536);
        encoder.u32(8_192);
        encoder.u8(0);
        encoder.u8(0);
        encoder.u8(0);
        encoder.u32(1);
        encoder.string("itemCount");
        encoder.u8(1);
        encoder.u64(2);
        encoder.u64(1);
        encoder.string("en");
        encoder.u32(0);
        encoder.into_bytes()
    }

    fn diverse_request(grammar: u32, session: u32, repetition: u32) -> Vec<u8> {
        let mut encoder = Encoder::new();
        encoder.u32(WIRE_VERSION);
        encoder.u32(grammar);
        encoder.u64(0);
        encoder.u8(0);
        encoder.u32(80);
        encoder.u32(2_000);
        encoder.u32(16_384);
        encoder.u32(65_536);
        encoder.u32(8_192);
        encoder.u8(0);
        encoder.u8(0);
        encoder.u8(0);
        encoder.u32(0);
        encoder.u32(session);
        encoder.u32(repetition);
        encoder.u8(0);
        encoder.into_bytes()
    }

    fn generation_request(grammar: u32) -> Vec<u8> {
        let mut encoder = Encoder::new();
        encoder.u32(WIRE_VERSION);
        encoder.u32(grammar);
        encoder.u64(0);
        encoder.u8(0);
        encoder.u32(80);
        encoder.u32(2_000);
        encoder.u32(16_384);
        encoder.u32(65_536);
        encoder.u32(8_192);
        encoder.into_bytes()
    }

    #[test]
    fn reports_linked_versions() {
        assert_eq!(meco_abi_version(), ABI_VERSION);
        assert_eq!(meco_core_api_version(), mecojoni_core::API_VERSION);
    }

    #[test]
    fn package_compile_generate_and_dispose_use_monotonic_handles() {
        let mut state = State::new();
        let package_result = dispatch(&mut state, OP_PACKAGE_CREATE, &package_request());
        assert_eq!(
            state.result(package_result).expect("result").status,
            STATUS_SUCCESS
        );
        let package = state
            .claim_result_value(package_result)
            .expect("package value");
        let compile_result = dispatch(&mut state, OP_COMPILE, &handle_request(package));
        let grammar = state
            .claim_result_value(compile_result)
            .expect("grammar value");
        let generation_result = dispatch(
            &mut state,
            OP_GENERATE_WEIGHTED,
            &generation_request(grammar),
        );

        assert_eq!(
            state.result(generation_result).expect("result").status,
            STATUS_SUCCESS
        );
        let generation_payload = &state.result(generation_result).expect("result").payload;
        let mut legacy = Decoder::new(generation_payload);
        assert_eq!(legacy.u32(), Ok(WIRE_VERSION));
        assert_eq!(legacy.u32(), Ok(PAYLOAD_GENERATE));
        assert!(legacy.string(1_024).is_ok());
        assert!(legacy.string(1_024).is_ok());
        assert!(legacy.u32().is_ok());
        assert!(legacy.u32().is_ok());
        assert_eq!(legacy.finish(), Ok(()));
        assert!(generation_result > compile_result && compile_result > package_result);
        for handle in [
            package_result,
            package,
            compile_result,
            grammar,
            generation_result,
        ] {
            assert!(state.dispose(handle));
            assert!(!state.dispose(handle));
        }
        assert!(state.handles.is_empty());
    }

    #[test]
    fn manifest_compile_and_structural_generation_return_typed_message_request() {
        let mut state = State::new();
        let package_result = dispatch(&mut state, OP_PACKAGE_CREATE, &localized_package_request());
        let package = state
            .claim_result_value(package_result)
            .expect("package handle");
        let compile_result = dispatch(
            &mut state,
            OP_COMPILE_WITH_MANIFEST,
            &compile_manifest_request(package),
        );
        assert_eq!(
            state.result(compile_result).expect("compile result").status,
            STATUS_SUCCESS
        );
        let grammar = state
            .claim_result_value(compile_result)
            .expect("grammar handle");
        let generation = dispatch(
            &mut state,
            OP_GENERATE_STRUCTURAL,
            &structural_request(grammar),
        );
        let record = state.result(generation).expect("structural result");
        assert_eq!(record.status, STATUS_SUCCESS);
        let mut decoder = Decoder::new(&record.payload);
        assert_eq!(decoder.u32(), Ok(WIRE_VERSION));
        assert_eq!(decoder.u32(), Ok(PAYLOAD_STRUCTURAL));
        assert_eq!(decoder.u8(), Ok(1));
        assert_eq!(decoder.string(64), Ok("arrival".to_string()));
        assert_eq!(decoder.u32(), Ok(2));
        assert_eq!(decoder.string(64), Ok("hero".to_string()));
        assert_eq!(decoder.u8(), Ok(0));
        assert_eq!(decoder.string(64), Ok("Ada".to_string()));
        assert_eq!(decoder.string(64), Ok("count".to_string()));
        assert_eq!(decoder.u8(), Ok(1));
        assert_eq!(decoder.u64(), Ok(2));
        assert_eq!(decoder.u64(), Ok(1));
        assert_eq!(decoder.string(64), Ok("en".to_string()));
        assert_eq!(decoder.u32(), Ok(0));
    }

    #[test]
    fn diverse_state_handles_generate_one_transactional_result() {
        let mut state = State::new();
        let package_result = dispatch(&mut state, OP_PACKAGE_CREATE, &package_request());
        let package = state
            .claim_result_value(package_result)
            .expect("package handle");
        let compile_result = dispatch(&mut state, OP_COMPILE, &handle_request(package));
        let grammar = state
            .claim_result_value(compile_result)
            .expect("grammar handle");
        let repetition_result = dispatch(
            &mut state,
            OP_REPETITION_CREATE,
            &WIRE_VERSION.to_le_bytes(),
        );
        let repetition = state
            .claim_result_value(repetition_result)
            .expect("repetition handle");
        let mut session_request = Encoder::new();
        session_request.u32(WIRE_VERSION);
        session_request.u64(0);
        let session_result = dispatch(&mut state, OP_SESSION_CREATE, &session_request.into_bytes());
        let session = state
            .claim_result_value(session_result)
            .expect("session handle");
        let generated = dispatch(
            &mut state,
            OP_GENERATE_DIVERSE,
            &diverse_request(grammar, session, repetition),
        );
        let record = state.result(generated).expect("diverse result");
        assert_eq!(record.status, STATUS_SUCCESS);
        let mut decoder = Decoder::new(&record.payload);
        assert_eq!(decoder.u32(), Ok(WIRE_VERSION));
        assert_eq!(decoder.u32(), Ok(PAYLOAD_DIVERSE));
        assert_eq!(decoder.string(64), Ok("hello".to_string()));

        let session_export = dispatch(
            &mut state,
            OP_SESSION_SNAPSHOT_EXPORT,
            &handle_request(session),
        );
        let repetition_export = dispatch(
            &mut state,
            OP_REPETITION_SNAPSHOT_EXPORT,
            &handle_request(repetition),
        );
        let snapshot_bytes = |state: &State, result: u32| {
            let mut decoder = Decoder::new(&state.result(result).expect("snapshot result").payload);
            assert_eq!(decoder.u32(), Ok(WIRE_VERSION));
            assert_eq!(decoder.u32(), Ok(PAYLOAD_SNAPSHOT));
            decoder.bytes(MAX_SNAPSHOT_BYTES).expect("snapshot bytes")
        };
        let session_bytes = snapshot_bytes(&state, session_export);
        let repetition_bytes = snapshot_bytes(&state, repetition_export);
        let import_request = |bytes: &[u8]| {
            let mut encoder = Encoder::new();
            encoder.u32(WIRE_VERSION);
            encoder.bytes(bytes);
            encoder.into_bytes()
        };
        let restored_session_result = dispatch(
            &mut state,
            OP_SESSION_SNAPSHOT_IMPORT,
            &import_request(&session_bytes),
        );
        let restored_session = state
            .claim_result_value(restored_session_result)
            .expect("restored session");
        let restored_repetition_result = dispatch(
            &mut state,
            OP_REPETITION_SNAPSHOT_IMPORT,
            &import_request(&repetition_bytes),
        );
        let restored_repetition = state
            .claim_result_value(restored_repetition_result)
            .expect("restored repetition");
        let original_next = dispatch(
            &mut state,
            OP_GENERATE_DIVERSE,
            &diverse_request(grammar, session, repetition),
        );
        let restored_next = dispatch(
            &mut state,
            OP_GENERATE_DIVERSE,
            &diverse_request(grammar, restored_session, restored_repetition),
        );
        assert_eq!(
            state.result(original_next).expect("original next").payload,
            state.result(restored_next).expect("restored next").payload
        );
    }

    #[test]
    fn disposing_an_unclaimed_result_disposes_its_value() {
        let mut state = State::new();
        let value = state
            .insert(HandleValue::Result(super::ResultRecord {
                status: STATUS_SUCCESS,
                value_handle: 0,
                value_claimed: false,
                payload: vec![],
            }))
            .expect("value handle");
        let result = state.add_result(super::ResultRecord {
            status: STATUS_SUCCESS,
            value_handle: value,
            value_claimed: false,
            payload: vec![],
        });

        assert!(state.dispose(result));
        assert!(state.get(value).is_none());
        assert!(state.handles.is_empty());
    }

    #[test]
    fn stale_and_cross_kind_handles_return_structured_errors() {
        let mut state = State::new();
        let package_result = dispatch(&mut state, OP_PACKAGE_CREATE, &package_request());
        let cross_kind = dispatch(&mut state, OP_COMPILE, &handle_request(package_result));
        let stale_result = dispatch(&mut state, OP_COMPILE, &handle_request(u32::MAX));

        for result in [cross_kind, stale_result] {
            let record = state.result(result).expect("error result");
            assert_eq!(record.status, STATUS_ERROR);
            let mut decoder = Decoder::new(&record.payload);
            assert_eq!(decoder.u32(), Ok(WIRE_VERSION));
            assert_eq!(decoder.u32(), Ok(PAYLOAD_ERROR));
        }
        assert!(matches!(
            state.get(package_result),
            Some(value) if value.kind() == HandleKind::Result
        ));
    }

    #[test]
    fn handle_ids_are_never_reused_and_exhaustion_is_bounded() {
        let mut state = State::new();
        state.next_handle = u32::MAX;
        let last = state.insert(HandleValue::Result(super::ResultRecord {
            status: STATUS_SUCCESS,
            value_handle: 0,
            value_claimed: false,
            payload: vec![],
        }));
        assert_eq!(last, Some(u32::MAX));
        assert!(state.remove(u32::MAX).is_some());
        assert_eq!(
            state.insert(HandleValue::Result(super::ResultRecord {
                status: STATUS_SUCCESS,
                value_handle: 0,
                value_claimed: false,
                payload: vec![],
            })),
            None
        );
    }

    #[test]
    fn wire_errors_are_result_payloads_not_panics() {
        let mut state = State::new();
        let result = dispatch(&mut state, OP_PACKAGE_CREATE, &[0, 1]);
        let record = state.result(result).expect("wire error result");
        let mut decoder = Decoder::new(&record.payload);

        assert_eq!(record.status, STATUS_ERROR);
        assert_eq!(decoder.u32(), Ok(WIRE_VERSION));
        assert_eq!(decoder.u32(), Ok(PAYLOAD_ERROR));
        assert_eq!(decoder.u32(), Ok(1));
        assert_eq!(decoder.string(64), Ok("E_ABI_WIRE_TRUNCATED".to_string()));
    }
}
