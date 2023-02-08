//! An implementation of the Cairo virtual machine
//!
//! # Feature Flags
//! - `skip_next_instruction_hint`: Enable the `skip_next_instruction()` hint. Not enabled by default.
//! - `hooks`: Enable [Hooks](vm::hooks) support for the [VirtualMachine](vm::vm_core::VirtualMachine). Not enabled by default.
//! - `with_mimalloc`: Use [MiMalloc](https://crates.io/crates/mimalloc) as the program global allocator.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(warnings)]
#![cfg_attr(any(target_arch = "wasm32", not(feature = "std")), no_std)]

#[cfg(feature = "std")]
include!("./with_std.rs");

#[cfg(all(not(feature = "std"), feature = "alloc"))]
include!("./without_std.rs");

pub mod cairo_run;
pub mod hint_processor;
pub mod math_utils;
pub mod serde;
pub mod types;
pub mod utils;
pub mod vm;

#[cfg(test)]
mod tests;

/// Feature gate some code that should only be run when `std` feature is enabled.
///
/// # Example
///
/// ```
/// use sp_std::if_std;
///
/// if_std! {
///     // This code is only being compiled and executed when the `std` feature is enabled.
///     println!("Hello native world");
/// }
/// ```
#[cfg(feature = "std")]
#[macro_export]
macro_rules! if_std {
	( $( $code:tt )* ) => {
		$( $code )*
	}
}

#[cfg(all(not(feature = "std"), feature = "alloc"))]
#[macro_export]
macro_rules! if_std {
    ( $( $code:tt )* ) => {};
}

pub mod prelude {
    pub use crate::{
        borrow::ToOwned,
        boxed::Box,
        clone::Clone,
        cmp::{Eq, PartialEq, Reverse},
        iter::IntoIterator,
        string::{String, ToString},
        vec::Vec,
    };
}
