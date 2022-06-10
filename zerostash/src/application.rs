//! Zerostash Abscissa Application

use crate::{commands::EntryPoint, prelude::*};
use abscissa_core::{
    application::{self, AppCell},
    config::{self, CfgCell},
    status_err, trace, Application, FrameworkError, StandardPaths,
};
use abscissa_tokio::TokioComponent;
use anyhow::Result;
use std::{num::NonZeroUsize, process};

/// Application state
pub static APP: AppCell<ZerostashApp> = AppCell::new();

/// Zerostash Application
#[derive(Debug)]
pub struct ZerostashApp {
    /// Application configuration.
    config: CfgCell<ZerostashConfig>,

    /// Application state.
    state: application::State<Self>,
}

/// Initialize a new application instance.
///
/// By default no configuration is loaded, and the framework state is
/// initialized to a default, empty state (no components, threads, etc).
impl Default for ZerostashApp {
    fn default() -> Self {
        Self {
            config: CfgCell::default(),
            state: application::State::default(),
        }
    }
}

impl Application for ZerostashApp {
    /// Entrypoint command for this application.
    type Cmd = EntryPoint;

    /// Application configuration.
    type Cfg = ZerostashConfig;

    /// Paths to resources within the application.
    type Paths = StandardPaths;

    /// Accessor for application configuration.
    fn config(&self) -> config::Reader<ZerostashConfig> {
        self.config.read()
    }

    /// Borrow the application state immutably.
    fn state(&self) -> &application::State<Self> {
        &self.state
    }

    /// Register all components used by this application.
    ///
    /// If you would like to add additional components to your application
    /// beyond the default ones provided by the framework, this is the place
    /// to do so.
    fn register_components(&mut self, command: &Self::Cmd) -> Result<(), FrameworkError> {
        let mut framework_components = self.framework_components(command)?;
        framework_components.push(Box::new(TokioComponent::new()?));
        let mut app_components = self.state.components_mut();
        app_components.register(framework_components)
    }

    /// Post-configuration lifecycle callback.
    ///
    /// Called regardless of whether config is loaded to indicate this is the
    /// time in app lifecycle when configuration would be loaded if
    /// possible.
    fn after_config(&mut self, config: Self::Cfg) -> Result<(), FrameworkError> {
        let mut components = self.state.components_mut();
        components.after_config(&config)?;
        self.config.set_once(config);
        Ok(())
    }

    /// Get tracing configuration from command-line options
    fn tracing_config(&self, command: &EntryPoint) -> trace::Config {
        match command.verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        }
        .to_owned()
        .into()
    }
}

impl ZerostashApp {
    #[allow(clippy::redundant_closure)]
    pub(crate) fn open_stash(&self, pathy: impl AsRef<str>) -> Stash {
        let stash = self.config().open(pathy);
        stash.unwrap_or_else(|e| fatal_error(e))
    }

    pub(crate) fn get_worker_threads(&self) -> usize {
        use std::cmp;
        cmp::min(
            std::thread::available_parallelism()
                .map(NonZeroUsize::get)
                .unwrap_or(1),
            16,
        )
    }
}

pub fn fatal_error(err: impl Into<Box<dyn std::error::Error>>) -> ! {
    status_err!("{} fatal error: {}", APP.name(), err.into());
    process::exit(1)
}
