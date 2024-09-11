use std::{io::BufRead, path::{Path, PathBuf}};

use xcb::{randr, xproto::Screen, Connection};
use serde::Deserialize;
use crate::error::Error;

#[derive(Deserialize, Debug)]
pub struct Display {
    #[allow(unused)] // TODO
    name: String,
    pub x: i32,
    pub y: i32,
    pub height: i32,
    pub width: i32,
}

pub fn xrandr_cache() -> Result<PathBuf, Error> {
    let xdg = PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?);
    Ok(xdg.join("xrandr-cache.json"))
}

pub fn read_displays_from_file<P: AsRef<Path>>(path: P) -> Result<Vec<Display>, Error> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut displays = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let display: Display = serde_json::from_str(&line)
            .expect("Failed to parse the line as a Display");
        displays.push(display);
    }

    Ok(displays)
}

pub(crate) fn get_screens(conn: &Connection, screen: &Screen)
     -> Result<Vec<Display>, Error>
{
    let mut screens = vec![];
    if let Ok(displays) = read_displays_from_file(&xrandr_cache()?) {
        return Ok(displays);
    }

    let window_dummy = conn.generate_id();

    xcb::create_window(
        &conn,
        0,
        window_dummy,
        screen.root(),
        0,
        0,
        1,
        1,
        0,
        0,
        0,
        &[],
    );
    conn.flush();

    let screen_res_cookie = randr::get_screen_resources(&conn, window_dummy);
    let screen_res_reply = screen_res_cookie.get_reply().unwrap();
    let crtcs = screen_res_reply.crtcs();

    let mut crtc_cookies = Vec::with_capacity(crtcs.len());
    for crtc in crtcs {
        crtc_cookies.push(randr::get_crtc_info(&conn, *crtc, 0));
    }

    for crtc_cookie in crtc_cookies.into_iter() {
        if let Ok(reply) = crtc_cookie.get_reply() {
            screens.push(Display {
                name: "".to_owned(),
                x: reply.x() as i32,
                y: reply.y() as i32,
                height: reply.height() as i32,
                width: reply.width() as i32,
            });
        }
    }

    xcb::destroy_window(&conn, window_dummy);

    Ok(screens)
}

pub(crate) fn get_largest_window(
    conn: &Connection,
    screen: &Screen,
) -> Result<((i32, i32), (i32, i32)), Error> {
    let screens = get_screens(conn, screen)?;
    let mut res = Err(Error::NoScreenFound);
    let mut size = 0;

    for screen in screens.into_iter() {
        let pixels = screen.width as u64 * screen.width as u64;
        if pixels > size {
            size = pixels;
            res = Ok(((screen.x, screen.y), (screen.width, screen.height)));
        }
    }

    res
}

pub(crate) fn create_gc_32(conn: &Connection, window: u32) -> Result<u32, Error> {
    // First create a dummy pixmap with 32 bit depth
    let pix32 = conn.generate_id();
    xcb::create_pixmap_checked(&conn, 32, pix32, window, 1, 1).request_check()?;

    // Then create a gc from that pixmap
    let gc = conn.generate_id();
    xcb::create_gc_checked(&conn, gc, pix32, &[]).request_check()?;

    // Free pixmap after creating the gc
    xcb::free_pixmap_checked(&conn, pix32).request_check()?;

    Ok(gc)
}
