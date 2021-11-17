use thiserror::Error;

#[derive(Error, Debug)]
pub(crate) enum Error {
    #[error("Io error; {0}")]
    IoError(#[from] std::io::Error),

    #[error("Fmt error; {0}")]
    FmtError(#[from] std::fmt::Error),

    #[error("Config error: {0}")]
    ConfigError(#[from] config::ConfigError),

    #[error("Serde yaml error; {0}")]
    SerdeYAMLError(#[from] serde_yaml::Error),

    #[error("ParseInt error: {0}")]
    ParseIntError(#[from] std::num::ParseIntError),

    #[error("Xcb error; {0}")]
    XcbError(#[from] xcb::Error<xcb::ffi::xcb_generic_error_t>),

    #[error("No screen found")]
    NoScreenFound,

    #[error("Invalid position specified")]
    InvalidPosition,

    #[error("Unknown key")]
    UnknownKey,

    #[error("Configuration not provided, run with --help")]
    NoConfig,
}
