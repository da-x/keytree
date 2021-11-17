use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;

pub type KeySym = u32;

static KEYSYMDEF: &'static str = include_str!("../inc/keysymdef.h");

lazy_static! {
    static ref RE: Regex =
        Regex::new(r"^[#]define[ \t]XK_([^ \t]+)[ \t]+0x([a-f0-9]+)(\t| |$)").unwrap();
}

lazy_static! {
    static ref MAP: (HashMap<KeySym, Arc<String>>, HashMap<Arc<String>, KeySym>) = {
        let mut m1 = HashMap::new();
        let mut m2 = HashMap::new();

        for line in KEYSYMDEF.lines() {
            if let Some(cap) = RE.captures(line) {
                let k = Arc::new(String::from(cap.get(1).unwrap().as_str()));
                let v = cap.get(2).unwrap().as_str();
                let z = u32::from_str_radix(v, 16).unwrap();

                if !m1.contains_key(&z) {
                    m1.insert(z, k.to_owned());
                }
                m2.insert(k.to_owned(), z);
            }
        }

        (m1, m2)
    };
}

pub fn is_modifier(k: KeySym) -> bool {
    let shift_l = name_to_sym("Shift_L").unwrap();
    let hyper_r = name_to_sym("Hyper_R").unwrap();
    let iso_lock = name_to_sym("ISO_Lock");
    let iso_level5_lock = name_to_sym("ISO_Level5_Lock");
    let mod_switch = name_to_sym("Mode_switch").unwrap();
    let xk_num_lock = name_to_sym("Num_Lock").unwrap();

    let iso = if let (Some(iso_lock), Some(iso_level5_lock)) = (iso_lock, iso_level5_lock) {
        (k >= iso_lock) && (k <= iso_level5_lock)
    } else {
        false
    };

    return ((k >= shift_l) && (k <= hyper_r)) || iso || (k == mod_switch) || (k == xk_num_lock);
}

pub fn sym_to_name(k: KeySym) -> Arc<String> {
    if let Some(name) = MAP.0.get(&k) {
        name.clone()
    } else {
        Arc::new(format!("<{}>", k))
    }
}

pub fn name_to_sym(name: &str) -> Option<KeySym> {
    if let Some(sym) = MAP.1.get(&Arc::new(name.to_owned())) {
        Some(*sym)
    } else {
        None
    }
}
