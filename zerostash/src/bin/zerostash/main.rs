//! Main entry point for Zerostash

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]
#![forbid(unsafe_code)]

use zerostash::application::APPLICATION;

/// Boot Zerostash
fn main() {
    abscissa_core::boot(&APPLICATION);
}
