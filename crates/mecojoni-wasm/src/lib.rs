#![cfg_attr(target_arch = "wasm32", no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

/// Version of the handwritten linear-memory ABI.
pub const ABI_VERSION: u32 = 1;

/// Returns the ABI version before any allocation or handle operation is attempted.
///
/// Rust 2024 classifies symbol export attributes as unsafe because duplicate
/// linker symbols cannot be diagnosed by Rust. Keep this exception local to
/// the ABI boundary; the implementation itself contains no unsafe block.
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

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::{ABI_VERSION, meco_abi_version, meco_core_api_version};

    #[test]
    fn reports_linked_versions() {
        assert_eq!(meco_abi_version(), ABI_VERSION);
        assert_eq!(meco_core_api_version(), mecojoni_core::API_VERSION);
    }
}
