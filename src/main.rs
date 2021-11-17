#[macro_use]
extern crate error_chain;

use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use xcb::Connection;

mod action;
mod cmdline;
mod combination;
mod config;
mod error;
mod keysym;
mod leechbar;
mod window;

use crate::action::Op;
use crate::combination::{Combination, Modifiers};
use crate::config::Config;
use crate::error::Error;
use crate::keysym::KeySym;
use crate::window::Window;
use ::config as config_crate;

struct Main {
    border_size: u16,
    border_pad: u16,
    screen_num: i32,
    format24: u32,
    format32: u32,
    frame: u32,
    black: u32,

    meta_mod_mask: xcb::ModMask,
    alt_mod_mask: xcb::ModMask,
    super_mod_mask: xcb::ModMask,
    hyper_mod_mask: xcb::ModMask,
    num_lock_mask: xcb::ModMask,
    scroll_lock_mask: xcb::ModMask,

    keycode_to_keysym: Vec<KeySym>,
    keysym_to_keycode: HashMap<KeySym, u8>,

    largest_window: ((i16, i16), (u16, u16)),
    pango_font: pango::FontDescription,
    conn: Arc<Connection>,
    opt: cmdline::Opt,
    config: Config,
}

enum KeyGrabbing {
    Grab,
    #[allow(unused)]
    Ungrab,
}

impl Main {
    fn key_grabbing(
        &self,
        key_sym: KeySym,
        modifiers: Modifiers,
        grabbing: KeyGrabbing,
    ) -> Result<(), Error> {
        let conn = self.conn.clone();
        let setup = conn.get_setup();
        let screen = setup.roots().nth(self.screen_num as usize).unwrap();
        let root = screen.root();

        let keycode = *self.keysym_to_keycode.get(&key_sym).unwrap();

        let mut mod_list: [u32; 8] = [0; 8];
        let modifiers = self.mask_to_x11(modifiers);

        // Create all combinations of the ignored modifiers
        mod_list[0] = 0;
        mod_list[1] = xcb::MOD_MASK_LOCK;
        mod_list[2] = self.num_lock_mask;
        mod_list[3] = mod_list[1] | mod_list[2];
        mod_list[4] = self.scroll_lock_mask;
        mod_list[5] = mod_list[1] | mod_list[4];
        mod_list[6] = mod_list[2] | mod_list[4];
        mod_list[7] = mod_list[1] | mod_list[2] | mod_list[4];

        // And grab all of them, so in effect the total grab
        // ignores those modifiers
        for i in 0..8 {
            match grabbing {
                KeyGrabbing::Grab => {
                    let data = xcb::grab_key(
                        &conn,
                        true,
                        root,
                        (modifiers | mod_list[i]) as u16,
                        keycode,
                        xcb::GRAB_MODE_ASYNC as u8,
                        xcb::GRAB_MODE_ASYNC as u8,
                    );
                    data.request_check()?;
                }
                KeyGrabbing::Ungrab => {
                    let data =
                        xcb::ungrab_key(&conn, keycode, root, (modifiers | mod_list[i]) as u16);
                    data.request_check()?;
                }
            }
        }

        Ok(())
    }

    fn load_keycode_to_keysyms(&mut self) -> Result<(), Error> {
        let setup = self.conn.get_setup();
        let data = xcb::get_keyboard_mapping(
            &self.conn,
            setup.min_keycode(),
            setup.max_keycode() - setup.min_keycode() + 1,
        );
        let r = data.get_reply()?;
        let n = r.keysyms_per_keycode();
        let mut keysym_idx = 0;
        let mut keycode_idx = setup.min_keycode();
        self.keycode_to_keysym.resize(keycode_idx as usize, 0);

        for keysym in r.keysyms() {
            if keysym_idx == 0 {
                // Group 0 & Shift 0
                self.keycode_to_keysym.push(*keysym);
                self.keysym_to_keycode.insert(*keysym, keycode_idx);
            }

            keysym_idx += 1;
            if keysym_idx == n {
                keysym_idx = 0;
                if keycode_idx == 255 {
                    break;
                }
                keycode_idx += 1;
            }
        }

        Ok(())
    }

    fn load_modifier_maps(&mut self) -> Result<(), Error> {
        let data = xcb::get_modifier_mapping(&self.conn);
        let r = data.get_reply()?;
        let n = r.keycodes_per_modifier();
        let mut keycode_idx = 0;
        let mut modifier_idx = 0;
        let modmasks = [
            xcb::MOD_MASK_1,
            xcb::MOD_MASK_2,
            xcb::MOD_MASK_3,
            xcb::MOD_MASK_4,
            xcb::MOD_MASK_5,
        ];

        for keycode in r.keycodes() {
            if modifier_idx >= 3 {
                let name = keysym::sym_to_name(self.keycode_to_keysym[*keycode as usize] as u32);
                let m = modmasks[modifier_idx - 3];
                match name.as_str() {
                    "Meta_L" | "Meta_R" => self.meta_mod_mask |= m,
                    "Alt_L" | "Alt_R" => self.alt_mod_mask |= m,
                    "Super_L" | "Super_R" => self.super_mod_mask |= m,
                    "Hyper_L" | "Hyper_R" => self.hyper_mod_mask |= m,
                    "Num_Lock" => self.num_lock_mask |= m,
                    "Scroll_Lock" => self.scroll_lock_mask |= m,
                    _ => {}
                }
            }

            keycode_idx += 1;
            if keycode_idx == n {
                keycode_idx = 0;
                modifier_idx += 1;
            }
        }

        // Alt key translate to Meta keys if we can't find any Meta key
        if self.meta_mod_mask != 0 {
            self.meta_mod_mask = self.alt_mod_mask;
            self.alt_mod_mask = 0;
        }

        // Meta takes precedence over alt
        if (self.alt_mod_mask & self.meta_mod_mask) != 0 {
            self.alt_mod_mask &= !self.meta_mod_mask;
        }

        Ok(())
    }

    fn looping(&self) -> Result<(), Error> {
        let mut key_map = self.config.map.clone();

        // Grab root keys
        for (comb, _action) in self.config.map.iter() {
            let comb = Combination::parse(comb)?;
            println!("Grabbing {}", comb);
            self.key_grabbing(comb.key, comb.modifiers, KeyGrabbing::Grab)?;
        }

        let mut win: Option<Window> = None;
        let mut prev_focus = None;
        let mut running = true;

        while running || win.is_some() {
            std::thread::sleep(Duration::from_millis(10));

            let event = if let Some(event) = self.conn.poll_for_event() {
                event
            } else {
                continue;
            };

            let r = event.response_type() & !0x80;
            match r {
                xcb::CONFIGURE_NOTIFY => {
                    if let Some(win) = &win {
                        win.draw(&self.conn)?;
                    }
                }
                xcb::KEY_PRESS => {
                    let event: &xcb::KeyPressEvent = unsafe { xcb::cast_event(&event) };
                    let key = self.keycode_to_keysym[event.detail() as usize];

                    if keysym::is_modifier(key) {
                        continue;
                    }

                    let state = event.state() as u32
                        & !(xcb::MOD_MASK_LOCK | self.num_lock_mask | self.scroll_lock_mask);

                    let combination = Combination {
                        key,
                        modifiers: self.x11_to_mask(state),
                    };

                    let combination_str = format!("{}", combination);
                    log::info!("Received: {}", combination_str);

                    let mut revert = false;
                    let mut take_focus = None;

                    if let Some(desc) = key_map.get(&combination_str) {
                        if let Some(m) = desc.action.action_map() {
                            let mut display_text = String::new();
                            writeln!(&mut display_text, "Next keys:")?;
                            writeln!(&mut display_text, "")?;

                            let v: Vec<_> = m.iter().collect();
                            let mut by_title: Vec<_> = v
                                .iter()
                                .enumerate()
                                .map(|(idx, x)| (&x.1.title, idx))
                                .collect();
                            by_title.sort();

                            for (_, idx) in by_title.into_iter() {
                                let (key, value) = &v[idx];
                                if value.title.len() == 0 {
                                    writeln!(&mut display_text, "{}", key)?;
                                } else {
                                    writeln!(&mut display_text, "{} - {}", key, value.title)?;
                                }
                            }
                            take_focus = Some((m, display_text.trim().to_owned()));
                        } else {
                            for op in desc.action.to_op_list() {
                                log::info!("Action: {:?}", op);

                                match op {
                                    Op::Execute(e) => {
                                        let mut cmd = std::process::Command::new("sh");
                                        cmd.arg("-c");
                                        cmd.arg(e);
                                        cmd.spawn()?;
                                    }
                                    Op::Reload(_) => {
                                        // TODO
                                    }
                                    Op::Die(_) => {
                                        running = false;
                                    }
                                }
                            }
                            revert = true;
                        }
                    } else {
                        revert = true;
                    }

                    if revert {
                        if let Some(win) = &win {
                            log::debug!("Destroying window");
                            win.destroy(&self.conn)?;
                        }
                        key_map = self.config.map.clone();
                    } else if let Some((take_focus, display_text)) = take_focus {
                        if let Some(win) = &mut win {
                            win.update(self, &display_text)?;
                        } else {
                            let data = xcb::get_input_focus(&self.conn);
                            let r = data.get_reply()?;
                            prev_focus = Some((r.focus(), r.revert_to()));
                            win = Some(Window::new(self, &display_text)?);
                        }
                        key_map = take_focus.clone();
                    }
                }
                xcb::DESTROY_NOTIFY => {
                    if let Some((focus, revert)) = prev_focus {
                        log::debug!("Reverting focus");

                        xcb::set_input_focus(&self.conn, revert, focus, xcb::CURRENT_TIME)
                            .request_check()?;
                        self.conn.flush();
                    }

                    win = None;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn mask_to_x11(&self, modifiers: Modifiers) -> xcb::ModMask {
        let mut m = 0;

        if modifiers.meta {
            m |= self.meta_mod_mask;
        }
        if modifiers.alt {
            m |= self.alt_mod_mask;
        }
        if modifiers.superr {
            m |= self.super_mod_mask;
        }
        if modifiers.hyper {
            m |= self.hyper_mod_mask;
        }
        if modifiers.control {
            m |= xcb::MOD_MASK_CONTROL;
        }

        m
    }

    fn new(opt: &cmdline::Opt, config: Config) -> Result<Self, Error> {
        let (conn, screen_num) = xcb::Connection::connect(None).unwrap();
        let conn = Arc::new(conn);
        let setup = conn.get_setup();
        let screen = setup.roots().nth(screen_num as usize).unwrap();
        let foreground = conn.generate_id();
        let frame = conn.generate_id();
        let black = conn.generate_id();
        let pango_font = pango::FontDescription::from_string(&opt.font);

        let (format24, format32) = leechbar::util::formats::image_formats(&conn);
        let border_size = 1;
        let border_pad = 10;

        xcb::create_gc(
            &conn,
            foreground,
            screen.root(),
            &[
                (xcb::GC_FOREGROUND, screen.white_pixel()),
                (xcb::GC_GRAPHICS_EXPOSURES, 0),
            ],
        )
        .request_check()?;

        xcb::create_gc(
            &conn,
            frame,
            screen.root(),
            &[
                (xcb::GC_FOREGROUND, screen.white_pixel()),
                (xcb::GC_GRAPHICS_EXPOSURES, 0),
            ],
        )
        .request_check()?;

        xcb::create_gc(
            &conn,
            black,
            screen.root(),
            &[
                (xcb::GC_FOREGROUND, screen.black_pixel()),
                (xcb::GC_GRAPHICS_EXPOSURES, 0),
            ],
        )
        .request_check()?;

        let largest_window = crate::leechbar::util::window::get_largest_window(&conn, &screen)?;

        Ok(Self {
            keycode_to_keysym: vec![],
            keysym_to_keycode: HashMap::new(),
            meta_mod_mask: 0,
            alt_mod_mask: 0,
            super_mod_mask: 0,
            hyper_mod_mask: 0,
            num_lock_mask: 0,
            scroll_lock_mask: 0,
            frame,
            black,
            format32,
            format24,
            screen_num,
            border_size,
            border_pad,
            largest_window,
            conn,
            pango_font,
            opt: opt.clone(),
            config,
        })
    }

    fn x11_to_mask(&self, mask: xcb::ModMask) -> Modifiers {
        Modifiers {
            control: mask & xcb::MOD_MASK_CONTROL != 0,
            alt: mask & self.alt_mod_mask != 0,
            hyper: mask & self.hyper_mod_mask != 0,
            superr: mask & self.super_mod_mask != 0,
            meta: mask & self.meta_mod_mask != 0,
        }
    }
}

fn main_wrap() -> Result<(), Error> {
    let opt = cmdline::Opt::from_args();
    if opt.example_config {
        print!("{}", serde_yaml::to_string(&config::example())?);
        return Ok(());
    }

    let config_path = if let Some(config) = &opt.config {
        config.clone()
    } else {
        if let Ok(path) = std::env::var("KITAP_CONFIG_PATH") {
            PathBuf::from(path)
        } else {
            if let Some(dir) = dirs::config_dir() {
                dir.join("keytree").join("config.yaml")
            } else {
                return Err(Error::NoConfig);
            }
        }
    };

    let file = config_crate::File::new(
        config_path.to_str().unwrap(),
        config_crate::FileFormat::Yaml,
    );
    let mut settings = config_crate::Config::default();
    settings.merge(file)?;
    let start_config = settings.try_into::<Config>()?;
    let mut main = Main::new(&opt, start_config)?;

    main.load_keycode_to_keysyms()?;
    main.load_modifier_maps()?;

    // Main loop
    main.looping()?;

    Ok(())
}

fn main() {
    match main_wrap() {
        Ok(()) => {}
        Err(Error::ConfigError(e)) => {
            eprintln!("Error: {}", e);
            eprintln!("See a sample config file with --show-example-config");
            std::process::exit(-1);
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(-1);
        }
    }
}
