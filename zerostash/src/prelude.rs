//! Application-local prelude: conveniently import types/functions/macros
//! which are generally useful and should be available in every module with
//! `use crate::prelude::*;

pub use crate::application::APP;
pub use crate::config::ZerostashConfig;
/// Application state
pub use abscissa_core::Application;

pub type Stash = infinitree::Infinitree<zerostash_files::Files>;

pub use async_trait::async_trait;
pub use clap::Parser as Command;

#[async_trait]
pub trait AsyncRunnable {
    async fn run(&self);
}
