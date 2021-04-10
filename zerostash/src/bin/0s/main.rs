//! Main entry point for Zerostash

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]
#![forbid(unsafe_code)]

/// Boot Zerostash
fn main() {
    abscissa_core::boot(&zerostash::application::APP);
}
