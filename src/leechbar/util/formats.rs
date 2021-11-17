use std::sync::Arc;

pub fn image_formats(conn: &Arc<xcb::Connection>) -> (u32, u32) {
    // Query connection for all available formats
    let formats = xcb::render::query_pict_formats(conn)
        .get_reply()
        .expect("Unable to query picture formats")
        .formats();

    let mut format24 = None;
    let mut format32 = None;
    for fmt in formats {
        let direct = fmt.direct();

        // Update 32 bit format if the format matches
        if fmt.depth() == 32
            && direct.alpha_shift() == 24
            && direct.red_shift() == 16
            && direct.green_shift() == 8
            && direct.blue_shift() == 0
        {
            format32 = Some(fmt);
        }

        // Update 24 bit format if the format matches
        if fmt.depth() == 24
            && direct.red_shift() == 16
            && direct.green_shift() == 8
            && direct.blue_shift() == 0
        {
            format24 = Some(fmt);
        }

        // Stop iteration when matches have been found
        if format32.is_some() && format24.is_some() {
            break;
        }
    }

    // Error if one of the formats hasn't been found
    match (format24, format32) {
        (Some(f_24), Some(f_32)) => (f_24.id(), f_32.id()),
        _ => panic!("Unable to find 32 or 24 depth picture formats"),
    }
}
