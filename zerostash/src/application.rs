//! Zerostash Abscissa Application

use crate::{
    commands::EntryPoint,
    config::{ask_credentials, ZerostashConfig},
};
use abscissa_core::{
    application::{self, AppCell},
    config::{self, CfgCell},
    status_err, trace, Application, FrameworkError, StandardPaths,
};
use abscissa_tokio::TokioComponent;

use anyhow::Result;
use libzerostash::Stash;

use std::{process, sync::Arc};

/// Application state
pub static APP: AppCell<ZerostashApp> = AppCell::new();

/// Zerostash Application
#[derive(Debug)]
pub struct ZerostashApp {
    /// Application configuration.
    pub config: CfgCell<ZerostashConfig>,

    /// Application state.
    pub state: application::State<Self>,
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

impl ZerostashApp {
    /// Open a stash or produce an error
    ///
    /// # Arguments
    ///
    /// * `pathy` - Can be a path or an alias stored in the config
    pub(crate) fn open_stash(&self, pathy: impl AsRef<str>) -> Stash {
        let config = self.config.read();

        let stash = match config.resolve_stash(&pathy) {
            None => {
                let path = pathy.as_ref();
                let key = ask_credentials().unwrap_or_else(|e| fatal_error(e));
                let backend = Arc::new(
                    libzerostash::backends::Directory::new(path).unwrap_or_else(|e| fatal_error(e)),
                );

                Stash::new(backend, key)
            }
            Some(cfg) => cfg.try_open().unwrap_or_else(|e| fatal_error(e)),
        };

        stash
    }

    pub(crate) async fn stash_exists(&self, pathy: impl AsRef<str>) -> Stash {
        let mut stash = self.open_stash(pathy);
        match stash.read().await {
            Ok(_) => stash,
            Err(e) => fatal_error(e),
        }
    }

    pub(crate) fn get_worker_threads(&self) -> usize {
        use std::cmp;
        cmp::min(num_cpus::get() + 1, 5)
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
        // Configure components
        let mut components = self.state.components_mut();
        components.after_config(&config)?;
        self.config.set_once(config);
        Ok(())
    }

    /// Get tracing configuration from command-line options
    fn tracing_config(&self, command: &EntryPoint) -> trace::Config {
        if command.verbose {
            trace::Config::verbose()
        } else {
            trace::Config::default()
        }
    }
}

/// report a fatal error and exit
pub fn fatal_error(err: impl Into<Box<dyn std::error::Error>>) -> ! {
    status_err!("{} fatal error: {}", APP.name(), err.into());
    process::exit(1)
}
