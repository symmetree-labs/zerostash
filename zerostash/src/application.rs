//! Zerostash Abscissa Application

use crate::{
    commands::ZerostashCmd,
    config::{ask_credentials, ZerostashConfig},
};
use abscissa_core::{
    application::{self, AppCell},
    config, status_err, status_warn, trace, Application, EntryPoint, FrameworkError, StandardPaths,
};
use anyhow::{format_err, Error, Result};
use libzerostash::Stash;

use std::{process, sync::Arc};

/// Application state
pub static APPLICATION: AppCell<ZerostashApp> = AppCell::new();

/// Obtain a read-only (multi-reader) lock on the application state.
///
/// Panics if the application state has not been initialized.
pub fn app_reader() -> application::lock::Reader<ZerostashApp> {
    APPLICATION.read()
}

/// Obtain an exclusive mutable lock on the application state.
pub fn app_writer() -> application::lock::Writer<ZerostashApp> {
    APPLICATION.write()
}

/// Obtain a read-only (multi-reader) lock on the application configuration.
///
/// Panics if the application configuration has not been loaded.
pub fn app_config() -> config::Reader<ZerostashApp> {
    config::Reader::new(&APPLICATION)
}

/// Zerostash Application
#[derive(Debug)]
pub struct ZerostashApp {
    /// Application configuration.
    config: Option<ZerostashConfig>,

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
            config: None,
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
        let config = &*app_config();

        let mut stash = match config.resolve_stash(&pathy) {
            None => {
                let path = pathy.as_ref();
                let key = ask_credentials().unwrap_or_else(|e| fatal_error(e));
                let backend = Arc::new(
                    libzerostash::backends::Directory::new(path)
                        .unwrap_or_else(|e| fatal_error(e.into())),
                );

                Stash::new(backend, key)
            }
            Some(cfg) => cfg.try_open().unwrap_or_else(|e| fatal_error(e)),
        };

        stash
    }

    pub(crate) fn stash_exists(&self, pathy: impl AsRef<str>) -> Stash {
        let mut stash = self.open_stash(pathy);
        match stash.read() {
            Ok(_) => stash,
            Err(e) => fatal_error2(e),
        }
    }

    pub(crate) fn get_worker_threads(&self) -> usize {
        use std::cmp;
        cmp::min(num_cpus::get() + 1, 5)
    }
}

impl Application for ZerostashApp {
    /// Entrypoint command for this application.
    type Cmd = EntryPoint<ZerostashCmd>;

    /// Application configuration.
    type Cfg = ZerostashConfig;

    /// Paths to resources within the application.
    type Paths = StandardPaths;

    /// Accessor for application configuration.
    fn config(&self) -> &ZerostashConfig {
        self.config.as_ref().expect("config not loaded")
    }

    /// Borrow the application state immutably.
    fn state(&self) -> &application::State<Self> {
        &self.state
    }

    /// Borrow the application state mutably.
    fn state_mut(&mut self) -> &mut application::State<Self> {
        &mut self.state
    }

    /// Register all components used by this application.
    ///
    /// If you would like to add additional components to your application
    /// beyond the default ones provided by the framework, this is the place
    /// to do so.
    fn register_components(&mut self, command: &Self::Cmd) -> Result<(), FrameworkError> {
        let components = self.framework_components(command)?;
        self.state.components.register(components)
    }

    /// Post-configuration lifecycle callback.
    ///
    /// Called regardless of whether config is loaded to indicate this is the
    /// time in app lifecycle when configuration would be loaded if
    /// possible.
    fn after_config(&mut self, config: Self::Cfg) -> Result<(), FrameworkError> {
        // Configure components
        self.state.components.after_config(&config)?;
        self.config = Some(config);
        Ok(())
    }

    /// Get tracing configuration from command-line options
    fn tracing_config(&self, command: &EntryPoint<ZerostashCmd>) -> trace::Config {
        if command.verbose {
            trace::Config::verbose()
        } else {
            trace::Config::default()
        }
    }
}

pub fn fatal_error2(err: Box<dyn std::error::Error>) -> ! {
    status_err!("{} fatal error: {}", (&*app_reader()).name(), err);
    process::exit(1)
}
pub fn fatal_error(err: Error) -> ! {
    status_err!("{} fatal error: {}", (&*app_reader()).name(), err);
    process::exit(1)
}
