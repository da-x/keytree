use super::super::util;
use super::super::util::color::Color;
use super::super::util::geometry::Geometry;
use super::picture::Picture;
use crate::leechbar::error::ErrorKind;
use cairo::{Context, Format, ImageSurface, Surface};
use pango::{FontDescription, Layout};
use std::sync::Arc;

/// A cached text.
///
/// This creates a text that is cached on the X server. Keeping this around instead of moving it
/// will usually lead to a lower CPU consumption but slightly increase the memory usage of the X
/// server.
#[derive(Clone)]
pub struct Text {
    pub(crate) arc: Arc<Picture>,
}

impl Text {
    /// Create a new cached text.
    ///
    /// This takes an optional font and color, if these are not set it will use the default font
    /// and color of the bar.
    ///
    /// # Errors
    ///
    /// This returns an error when the `content` parameter is an empty string slice, or when an
    /// X.Org request failed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use leechbar::{Text, BarBuilder};
    ///
    /// let bar = BarBuilder::new().spawn().unwrap();
    /// let text = Text::new(&bar, "Hello, World", None, None).unwrap();
    /// ```
    pub fn new(
        conn: Arc<xcb::Connection>,
        geometry: Geometry,
        gcontext: u32,
        window: u32,
        format32: u32,
        content: &str,
        font: &FontDescription,
        color: Color,
    ) -> Result<Self, ErrorKind> {
        // It's not possible to create an empty text
        // This returns an error if it is attempted
        if content.is_empty() {
            return Err("Text content empty".into());
        }

        // Close connection for destructor
        let conn = Arc::clone(&conn);

        // Get width and height for text
        let (w, h) = (text_width(content, font)?, geometry.height);
        // Create a new pixmap with empty background
        let pix = conn.generate_id();
        xtry!(create_pixmap_checked, &conn, 32, pix, window, w, h);
        let rect = &[xcb::Rectangle::new(0, 0, w, h)];

        xtry!(poly_fill_rectangle_checked, &conn, pix, gcontext, rect);

        // Create an xcb surface
        let mut visualtype = find_visualtype32(&util::screen(&conn)?)
            .ok_or_else(|| ErrorKind::ScreenDepthError(()))?;
        let surface = unsafe {
            Surface::from_raw_full(cairo_sys::cairo_xcb_surface_create(
                conn.get_raw_conn() as *mut cairo_sys::xcb_connection_t,
                pix,
                (&mut visualtype.base as *mut xcb::ffi::xcb_visualtype_t)
                    as *mut cairo_sys::xcb_visualtype_t,
                i32::from(w),
                i32::from(h),
            ))
        }
        .unwrap();

        // Create context and layout for drawing text
        let context = Context::new(&surface);
        let layout = layout(&context, content, font);

        // Set font color
        let color = color.as_fractions();
        context.set_source_rgba(color.0, color.1, color.2, color.3);

        // Center text horizontally and vertically
        let (_, text_height) = layout.get_pixel_size();
        let text_y = (f64::from(h) - f64::from(text_height)) / 2.;
        context.move_to(0., text_y);

        // Display text
        pangocairo::functions::show_layout(&context, &layout);

        // Create picture from pixmap
        let picture = conn.generate_id();
        xtry!(@render create_picture_checked, &conn, picture, pix, format32, &[]);

        // Free the unneeded pixmap
        xcb::free_pixmap(&conn, pix);

        Ok(Self {
            arc: Arc::new(Picture {
                conn,
                xid: picture,
                geometry: Geometry::new(0, 0, w, h),
            }),
        })
    }
}

// Get the width text will have with the specified font
fn text_width(text: &str, font: &FontDescription) -> Result<u16, ErrorKind> {
    // Create a dummy surface and context
    let surface = ImageSurface::create(Format::ARgb32, 0, 0)
        .map_err(|e| format!("Unable to create dummy layout for font size: {:?}", e))?;
    let context = Context::new(&surface);

    // Create the layout
    let layout = layout(&context, text, font);

    // Get the width of the text
    let width = layout.get_pixel_size().0;

    Ok(width as u16)
}

pub fn text_size(text: &str, font: &FontDescription) -> Result<(u16, u16), ErrorKind> {
    // Create a dummy surface and context
    let surface = ImageSurface::create(Format::ARgb32, 0, 0)
        .map_err(|e| format!("Unable to create dummy layout for font size: {:?}", e))?;
    let context = Context::new(&surface);

    // Create the layout
    let layout = layout(&context, text, font);

    // Get the width of the text
    let size = layout.get_pixel_size();

    Ok((size.0 as u16, size.1 as u16))
}

// Create a layout with the font and text
fn layout(context: &Context, text: &str, font: &FontDescription) -> Layout {
    let layout = pangocairo::functions::create_layout(context).expect("Unable to create layout.");
    layout.set_text(text);
    layout.set_font_description(Some(font));
    layout
}

// Get the first available visualtype with 32 bit depth
fn find_visualtype32<'s>(screen: &xcb::Screen<'s>) -> Option<xcb::Visualtype> {
    for depth in screen.allowed_depths() {
        if depth.depth() == 32 {
            let visual = depth.visuals().next();
            if let Some(visual) = visual {
                return Some(visual);
            }
        }
    }
    None
}
