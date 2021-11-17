use crate::error::Error;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug, Clone)]
pub(crate) struct Opt {
    #[structopt(name = "config-file", short = "c")]
    pub config: Option<PathBuf>,

    /// Font to use (Pango font string, for example "normal 100" for big text)
    #[structopt(long = "font", short = "n", default_value = "normal 25")]
    pub font: String,

    /// Initial screen position
    #[structopt(long = "position", short = "p", default_value = "%50,%50")]
    pub position: String,

    #[structopt(long = "show-example-config")]
    pub example_config: bool,
}

pub(crate) fn parse_position(v: &str, measure: u16, screen_measure: u16) -> Result<i16, Error> {
    let pos = if v.starts_with("%") {
        let percent: u64 = v[1..].parse()?;
        ((screen_measure as u64 * percent) / 100) as u16
    } else {
        v[..].parse()?
    };
    let pos = pos as i16 - measure as i16 / 2;
    let min = screen_measure.saturating_sub(measure) as i16;
    Ok(std::cmp::max(0, std::cmp::min(min, pos)))
}
