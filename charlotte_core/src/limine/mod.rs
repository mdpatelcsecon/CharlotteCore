//! # Limine Requests
//! This module contains requests for information from the Limine boot protocol.

use limine::request::*;
use limine::BaseRevision;

/// Require version 1 or later of the Limine boot protocol
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

/// This request is used to obtain a direct mapping of physical memory
/// in the kernel's address space.
pub static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();
/// This request is used to obtain the memory map.
pub static MEMMAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();