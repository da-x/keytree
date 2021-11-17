use crate::leechbar::component::text::Text;
use xcb::Connection;

use crate::error::Error;

pub(crate) fn draw_text_box(
    conn: &Connection,
    win: u32,
    frame: u32,
    black: u32,
    window_pict: u32,
    border_size: u16,
    border_pad: u16,
    total_width: u16,
    total_height: u16,
    text: &Text,
) -> Result<(), Error> {
    xcb::poly_fill_rectangle(
        &conn,
        win,
        black,
        &[xcb::Rectangle::new(
            0,
            0,
            total_width + (border_pad * 2 + border_size),
            total_height + (border_pad * 2 + border_size),
        )],
    );

    xcb::poly_rectangle(
        &conn,
        win,
        frame,
        &[xcb::Rectangle::new(
            0,
            0,
            total_width + (border_pad * 2 + border_size),
            total_height + (border_pad * 2 + border_size),
        )],
    );

    let op = xcb::render::PICT_OP_OVER as u8;
    let pw = text.arc.geometry.width;
    let ph = text.arc.geometry.height;

    xcb::render::composite_checked(
        &conn,
        op,
        text.arc.xid,
        0,
        window_pict,
        0,
        0,
        0,
        0,
        (border_pad + border_size) as i16,
        (border_pad + border_size) as i16,
        pw,
        ph,
    )
    .request_check()?;

    conn.flush();

    Ok(())
}
