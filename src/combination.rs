use crate::keysym::{self, KeySym};
use crate::Error;

pub type KeyCombination = String;

#[derive(Default)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
    pub hyper: bool,
    pub superr: bool,
}

pub struct Combination {
    pub key: KeySym,
    pub modifiers: Modifiers,
}

impl std::fmt::Display for Combination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.modifiers.control {
            write!(f, "C-")?;
        }
        if self.modifiers.superr {
            write!(f, "S-")?;
        }
        if self.modifiers.meta {
            write!(f, "M-")?;
        }
        if self.modifiers.alt {
            write!(f, "A-")?;
        }
        if self.modifiers.hyper {
            write!(f, "H-")?;
        }
        write!(f, "{}", keysym::sym_to_name(self.key))?;
        Ok(())
    }
}

impl Combination {
    pub(crate) fn parse(s: &str) -> Result<Self, Error> {
        let v: Vec<_> = s.split("-").collect();
        let mut mods = Modifiers::default();

        for i in 0..v.len() - 1 {
            match v[i] {
                "C" | "Ctrl" | "Control" => mods.control = true,
                "S" | "Sup" | "Super" => mods.superr = true,
                "M" | "Meta" => mods.meta = true,
                "A" | "Alt" => mods.alt = true,
                "H" | "Hyp" | "Hyper" => mods.hyper = true,
                _ => {}
            }
        }

        if let Some(key) = keysym::name_to_sym(v[v.len() - 1]) {
            Ok(Self {
                key,
                modifiers: mods,
            })
        } else {
            Err(Error::UnknownKey)
        }
    }
}
