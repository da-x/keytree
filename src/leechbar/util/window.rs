use xcb::{randr, xproto::Screen, Connection};

use crate::error::Error;

pub(crate) fn get_largest_window(
    conn: &Connection,
    screen: &Screen,
) -> Result<((i16, i16), (u16, u16)), Error> {
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

    let mut res = Err(Error::NoScreenFound);
    let mut size = 0 as u64;

    for crtc_cookie in crtc_cookies.into_iter() {
        if let Ok(reply) = crtc_cookie.get_reply() {
            let pixels = reply.width() as u64 * reply.height() as u64;
            if pixels > size {
                size = pixels;
                res = Ok(((reply.x(), reply.y()), (reply.width(), reply.height())));
            }
        }
    }

    xcb::destroy_window(&conn, window_dummy);

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
