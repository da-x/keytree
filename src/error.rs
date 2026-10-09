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

    #[error("No screen found")]
    NoScreenFound,

    #[error("Invalid position specified")]
    InvalidPosition,

    #[error("Unknown key: {0}")]
    UnknownKey(String),

    #[error("Var error: {0}")]
    VarError(#[from] std::env::VarError),

    #[error("Configuration not provided, run with --help")]
    NoConfig,

    #[error("Could not draw the overlay: {0}")]
    Draw(String),

    #[error("Wayland error: {0}")]
    Wayland(String),

    #[error(
        "This compositor does not offer zwlr_layer_shell_v1, which keytree needs to show its overlay.\n\
Supported compositors include Sway, Hyprland, Niri, River, Wayfire, and KWin."
    )]
    LayerShellMissing,

    #[error(
        "Several root keys are configured. Pass the one your shortcut should open with --root-key.\n  \
sway:     bindsym Menu exec keytree --root-key Menu\n  \
hyprland: bind = Menu, exec, keytree --root-key Menu"
    )]
    RootKeyRequired,
}
