pub mod color;
pub mod draw;
pub mod formats;
pub mod geometry;
pub mod window;

use super::error::*;
pub use color::Color;
pub use geometry::Geometry;
use std::sync::Arc;

// Get the screen from an XCB Connection
pub fn screen(conn: &Arc<xcb::Connection>) -> Result<xcb::Screen> {
    conn.get_setup()
        .roots()
        .next()
        .ok_or_else(|| ErrorKind::XcbNoScreenError(()).into())
}
