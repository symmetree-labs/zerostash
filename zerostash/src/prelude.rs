//! Application-local prelude: conveniently import types/functions/macros
//! which are generally useful and should be available in every module with
//! `use crate::prelude::*;

pub use crate::application::APP;
pub use crate::commands::{EntryPoint, StashArgs};
pub use crate::config::ZerostashConfig;
pub use abscissa_core::{status_err, Application};
pub use async_trait::async_trait;
pub use clap::Parser as Command;
pub use std::io::Write;

pub type Stash = infinitree::Infinitree<zerostash_files::Files>;

#[async_trait]
pub trait AsyncRunnable {
    async fn run(&self);
}

pub fn fatal_error(err: impl Into<Box<dyn std::error::Error>>) -> ! {
    status_err!("{} fatal error: {}", APP.name(), err.into());
    std::process::exit(1)
}
