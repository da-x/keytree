// Attempts an XCB operation and returns an error when it fails
macro_rules! xtry {
    ($func:ident, $($args:expr),*) => {
        {
            xcb::$func($($args),*)
                .request_check()
                .map_err(|e| crate::leechbar::error::ErrorKind::XError(e.error_code().to_string()))?;
        }

    };
    (@render $func:ident, $($args:expr),*) => {
        {
            xcb::render::$func($($args),*)
                .request_check()
                .map_err(|e| crate::leechbar::error::ErrorKind::XError(e.error_code().to_string()))?;
        }
    };
}
