use crate::error::Error;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug, Clone)]
pub(crate) struct Opt {
    #[structopt(long, short = "c")]
    pub config: Option<PathBuf>,

    /// Root key the compositor shortcut opens.
    ///
    /// Wayland clients cannot grab keys globally. Bind this key in the compositor
    /// so that it runs keytree. The process exits when the sequence finishes.
    /// If the configuration has a single root entry, this flag can be omitted.
    #[structopt(long, short = "r")]
    pub root_key: Option<String>,

    /// Font to use (Pango font string, for example "normal 100" for big text)
    #[structopt(long = "font", short = "n", default_value = "normal 25")]
    pub font: String,

    /// Card center inside the active workspace.
    ///
    /// `%50,%50` (the default) centers the card. A percentage is a fraction of
    /// that workspace axis. A plain number is a pixel offset of the card center.
    #[structopt(long = "position", short = "p", default_value = "%50,%50")]
    pub position: String,

    #[structopt(long = "show-example-config")]
    pub example_config: bool,
}

/// Offset of the card's top-left inside an axis of length `screen_measure`.
///
/// A `%N` value is N percent of the axis. Any other value is a pixel position.
/// The returned offset places the center of a card of size `measure` at that
/// point, then clamps the card so it stays inside the axis.
pub(crate) fn parse_position(v: &str, measure: i32, screen_measure: i32) -> Result<i32, Error> {
    let pos = if let Some(rest) = v.strip_prefix('%') {
        let percent: u64 = rest.parse()?;
        let computed = (screen_measure as i64).saturating_mul(percent as i64) / 100;
        saturating_i32(computed)
    } else {
        v.parse()?
    };
    let pos = pos.saturating_sub(measure / 2);
    let min = screen_measure.saturating_sub(measure).max(0);
    Ok(pos.clamp(0, min))
}

fn saturating_i32(value: i64) -> i32 {
    if value > i32::MAX as i64 {
        i32::MAX
    } else if value < i32::MIN as i64 {
        i32::MIN
    } else {
        value as i32
    }
}

#[cfg(test)]
mod tests {
    use super::parse_position;

    #[test]
    fn percent_centers_the_card() {
        assert_eq!(parse_position("%50", 10, 100).unwrap(), 45);
    }

    #[test]
    fn pixels_are_a_center_offset() {
        assert_eq!(parse_position("10", 10, 100).unwrap(), 5);
    }

    #[test]
    fn percent_clamps_inside_the_axis() {
        assert_eq!(parse_position("%0", 10, 100).unwrap(), 0);
        assert_eq!(parse_position("%100", 10, 100).unwrap(), 90);
    }
}
