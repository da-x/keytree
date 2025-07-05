use xcb::Connection;

use crate::error::Error;
use crate::leechbar::component::text::Text;
use crate::{cmdline, leechbar};

pub(crate) struct Window {
    id: u32,
    frame: u32,
    black: u32,
    window_pict: u32,
    border_size: u16,
    border_pad: u16,
    text_width: u16,
    text_height: u16,
    text: Text,
}

impl Window {
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    pub(crate) fn new(main: &crate::Main, text: &str) -> Result<Window, Error> {
        let conn = main.conn.clone();
        let setup = conn.get_setup();
        let screen = setup.roots().nth(main.screen_num as usize).unwrap();
        let largest_window = crate::leechbar::util::window::get_largest_window(&conn, &screen)?;

        let (text_width, text_height) =
            leechbar::component::text::text_size(&text, &main.pango_font).unwrap();
        let total_width = text_width + (main.border_pad + main.border_size) * 2;
        let total_height = text_height + (main.border_pad + main.border_size) * 2;
        let win = conn.generate_id();
        let (pos_x, pos_y) = if let Some((pos_x, pos_y)) = main.opt.position.split_once(",") {
            let x = cmdline::parse_position(pos_x, total_width, largest_window.1 .0 as u16)?;
            let y = cmdline::parse_position(pos_y, total_height, largest_window.1 .1 as u16)?;
            (
                (largest_window.0 .0 as i16 + x) as i16,
                (largest_window.0 .1 as i16 + y) as i16,
            )
        } else {
            return Err(Error::InvalidPosition);
        };

        xcb::create_window(
            &conn,
            xcb::WINDOW_CLASS_COPY_FROM_PARENT as u8,
            win,
            screen.root(),
            pos_x,
            pos_y,
            total_width,
            total_height,
            0,
            xcb::WINDOW_CLASS_INPUT_OUTPUT as u16,
            screen.root_visual(),
            &[
                (xcb::CW_BACK_PIXEL, screen.black_pixel()),
                (xcb::CW_OVERRIDE_REDIRECT, 1),
                (
                    xcb::CW_EVENT_MASK,
                    xcb::EVENT_MASK_EXPOSURE
                        | xcb::EVENT_MASK_STRUCTURE_NOTIFY
                        | xcb::EVENT_MASK_KEY_PRESS,
                ),
            ],
        )
        .request_check()?;

        conn.flush();

        let gcontext = leechbar::util::window::create_gc_32(&conn, win)?;
        let geometry = leechbar::util::Geometry::new(0, 0, text_width, text_height);
        let color = leechbar::util::Color::new(255, 255, 255, 255);
        let text = Text::new(
            conn.clone(),
            geometry,
            gcontext,
            win,
            main.format32,
            &text,
            &main.pango_font,
            color,
        )
        .unwrap();
        xcb::free_gc_checked(&conn, gcontext);

        let window_pict = conn.generate_id();
        xcb::render::create_picture_checked(&conn, window_pict, win, main.format24, &[])
            .request_check()?;

        xcb::map_window(&conn, win).request_check()?;
        conn.flush();

        let win = Window {
            id: win,
            frame: main.frame,
            black: main.black,
            window_pict,
            border_size: main.border_size,
            border_pad: main.border_pad,
            text_width,
            text_height,
            text: text.clone(),
        };

        // Poll for MAP_NOTIFY event
        loop {
            if let Some(event) = conn.wait_for_event() {
                let r = event.response_type() & !0x80;
                match r {
                    xcb::MAP_NOTIFY => {
                        break;
                    }
                    _ => {},
                }
            }
        }

        win.draw(&conn)?;
        conn.flush();

        xcb::set_input_focus(
            &conn,
            xcb::INPUT_FOCUS_POINTER_ROOT as u8,
            win.id,
            xcb::CURRENT_TIME,
        )
        .request_check()?;
        conn.flush();

        Ok(win)
    }

    pub(crate) fn update(&mut self, main: &crate::Main, text: &str) -> Result<(), Error> {
        let conn = main.conn.clone();
        let (text_width, text_height) =
            leechbar::component::text::text_size(&text, &main.pango_font).unwrap();
        let total_width = text_width + (main.border_pad + main.border_size) * 2;
        let total_height = text_height + (main.border_pad + main.border_size) * 2;

        let gcontext = leechbar::util::window::create_gc_32(&conn, self.id)?;
        let geometry = leechbar::util::Geometry::new(0, 0, text_width, text_height);
        let color = leechbar::util::Color::new(255, 255, 255, 255);
        let text = Text::new(
            conn.clone(),
            geometry,
            gcontext,
            self.id,
            main.format32,
            &text,
            &main.pango_font,
            color,
        )
        .unwrap();
        xcb::free_gc_checked(&conn, gcontext);

        self.text_width = text_width;
        self.text_height = text_height;
        self.text = text;

        xcb::configure_window(
            &conn,
            self.id,
            &[
                (xcb::CONFIG_WINDOW_WIDTH as u16, total_width as u32),
                (xcb::CONFIG_WINDOW_HEIGHT as u16, total_height as u32),
            ][..],
        );
        conn.flush();
        self.draw(&conn)?;

        Ok(())
    }

    pub(crate) fn draw(&self, conn: &Connection) -> Result<(), Error> {
        crate::leechbar::util::draw::draw_text_box(
            &conn,
            self.id,
            self.frame,
            self.black,
            self.window_pict,
            self.border_size,
            self.border_pad,
            self.text_width,
            self.text_height,
            &self.text,
        )?;

        Ok(())
    }

    pub(crate) fn destroy(&self, conn: &Connection) -> Result<(), Error> {
        xcb::render::free_picture_checked(conn, self.window_pict);
        xcb::destroy_window_checked(conn, self.id);
        conn.flush();
        Ok(())
    }
}
