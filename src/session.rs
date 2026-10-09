use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;

use config::builder::DefaultState;

use crate::action::{ActionDesc, Op};
use crate::cmdline::Opt;
use crate::combination::{Combination, KeyCombination};
use crate::config::Config;
use crate::error::Error;
use crate::keysym;

/// What the overlay should do after a key.
#[derive(Debug)]
pub(crate) enum Step {
    /// Replace the help text and keep the overlay open.
    Show(String),
    /// The sequence is finished. Close the overlay and exit.
    Close,
    /// Show `text` on the overlay, then exit after a short pause.
    Error(String),
    /// The key does not advance the sequence.
    Ignore,
}

pub(crate) struct Session {
    pub opt: Opt,
    config: Config,
    key_map: HashMap<KeyCombination, ActionDesc>,
    children: Vec<Child>,
}

impl Session {
    pub fn load(opt: &Opt) -> Result<Self, Error> {
        let config = load_config(opt)?;
        Ok(Self::from_parts(opt.clone(), config))
    }

    fn from_parts(opt: Opt, config: Config) -> Self {
        let key_map = config.map.clone();
        Self {
            opt,
            config,
            key_map,
            children: Vec::new(),
        }
    }

    /// The alias string to feed in as the opening key.
    pub fn resolve_root(&self) -> Result<String, Error> {
        if let Some(root) = &self.opt.root_key {
            let mut keys: Vec<&String> = self.config.map.keys().collect();
            keys.sort();
            for key in keys {
                for alias in key.split(',') {
                    if alias == root {
                        return Ok(alias.to_owned());
                    }
                }
            }
            return Err(Error::UnknownKey(root.clone()));
        }

        let mut keys: Vec<&String> = self.config.map.keys().collect();
        keys.sort();
        match keys.as_slice() {
            [only] => Ok(only.split(',').next().unwrap_or(only).to_owned()),
            [] => Err(Error::UnknownKey("(no keys in the map)".to_owned())),
            _ => Err(Error::RootKeyRequired),
        }
    }

    pub fn inject(&mut self, spec: &str) -> Result<Step, Error> {
        let combination = Combination::parse(spec)?;
        self.on_key(combination)
    }

    pub fn on_key(&mut self, combination: Combination) -> Result<Step, Error> {
        if keysym::is_modifier(combination.key) {
            return Ok(Step::Ignore);
        }
        // Escape dismisses even when a binding uses it.
        if combination.key == keysym::name_to_sym("Escape").unwrap_or(0xff1b) {
            return Ok(Step::Close);
        }

        let mut combination_str = format!("{}", combination);
        log::info!("Received: {}", combination_str);

        let keys: Vec<String> = self.key_map.keys().cloned().collect();
        'alias: for item in &keys {
            for alias in item.split(',') {
                if alias == combination_str {
                    combination_str = item.clone();
                    break 'alias;
                }
            }
        }

        let Some(desc) = self.key_map.get(&combination_str).cloned() else {
            return Ok(Step::Close);
        };

        if let Some(map) = desc.action.action_map() {
            let markup = list_markup(map);
            self.key_map = map.clone();
            return Ok(Step::Show(markup));
        }

        let mut failed = None;
        for op in desc.action.to_op_list() {
            log::info!("Action: {:?}", op);
            match op {
                Op::Execute(script) => self.execute(&script)?,
                Op::Reload(_) => match load_config(&self.opt) {
                    Ok(config) => {
                        self.config = config;
                        break;
                    }
                    Err(err) => {
                        failed = Some(err.to_string());
                        break;
                    }
                },
                Op::Die(_) => {}
            }
        }
        self.key_map = self.config.map.clone();
        if let Some(text) = failed {
            Ok(Step::Error(text))
        } else {
            Ok(Step::Close)
        }
    }

    fn execute(&mut self, script: &str) -> Result<(), Error> {
        let mut still_running = Vec::new();
        for mut child in self.children.drain(..) {
            if let Ok(Some(_)) = child.try_wait() {
                // The child has exited and was reaped.
            } else {
                still_running.push(child);
            }
        }
        let child = std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .spawn()?;
        still_running.push(child);
        self.children = still_running;
        Ok(())
    }
}

pub(crate) fn load_config(opt: &Opt) -> Result<Config, Error> {
    let config_path = if let Some(config) = &opt.config {
        config.clone()
    } else if let Ok(path) = std::env::var("KEYTREE_CONFIG_PATH") {
        PathBuf::from(path)
    } else if let Some(dir) = dirs::config_dir() {
        dir.join("keytree").join("config.yaml")
    } else {
        return Err(Error::NoConfig);
    };

    let file = config::File::new(config_path.to_str().unwrap(), config::FileFormat::Yaml);
    let settings = config::ConfigBuilder::<DefaultState>::default()
        .add_source(file)
        .add_source(config::Environment::with_prefix("KEYTREE_CONFIG_"))
        .build();
    let config = settings?.try_deserialize::<Config>()?;
    Ok(config)
}

pub(crate) fn error_markup(text: &str) -> String {
    format!(
        "<span foreground=\"#f2f4f8\">{}</span>",
        escape_markup(text)
    )
}

fn list_markup(map: &HashMap<KeyCombination, ActionDesc>) -> String {
    let mut entries: Vec<(&String, &ActionDesc)> = map.iter().collect();
    entries.sort_by(|a, b| a.1.title.cmp(&b.1.title).then_with(|| a.0.cmp(b.0)));

    let mut text = String::from("<span foreground=\"#9aa3b2\">Next keys:</span>\n");
    for (key, value) in entries {
        text.push('\n');
        text.push_str(&format!(
            "<span foreground=\"#f2f4f8\" weight=\"semibold\">{}</span>",
            escape_markup(key)
        ));
        if !value.title.is_empty() {
            text.push_str(&format!(
                "<span foreground=\"#c5cad3\">  —  {}</span>",
                escape_markup(&value.title)
            ));
        }
    }
    text
}

fn escape_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, ActionDesc};
    use crate::config::example;

    fn opt_with_root(root: Option<&str>) -> Opt {
        Opt {
            config: None,
            root_key: root.map(str::to_owned),
            font: "normal 25".to_owned(),
            position: "%50,%50".to_owned(),
            example_config: false,
        }
    }

    #[test]
    fn several_roots_require_the_flag() {
        let session = Session::from_parts(opt_with_root(None), example());
        assert!(matches!(
            session.resolve_root(),
            Err(Error::RootKeyRequired)
        ));
    }

    #[test]
    fn root_flag_selects_the_matching_alias() {
        let session = Session::from_parts(opt_with_root(Some("C-F6")), example());
        assert_eq!(session.resolve_root().unwrap(), "C-F6");
    }

    #[test]
    fn single_root_uses_its_first_alias() {
        let mut config = example();
        config.map.retain(|key, _| key == "C-c");
        let session = Session::from_parts(opt_with_root(None), config);
        assert_eq!(session.resolve_root().unwrap(), "C-c");
    }

    #[test]
    fn opening_a_map_lists_children_sorted_by_title() {
        let mut session = Session::from_parts(opt_with_root(Some("C-F6")), example());
        match session.inject("C-F6").unwrap() {
            Step::Show(markup) => {
                let reload = markup.find("Reload").unwrap();
                let files = markup.find("Open file manager").unwrap();
                let sub = markup.find("Sub actions").unwrap();
                assert!(files < reload && reload < sub);
                assert!(markup.contains("Next keys:"));
                assert!(markup.contains("#f2f4f8"));
            }
            other => panic!("expected a help list, got {:?}", other),
        }
    }

    #[test]
    fn escape_and_unknown_keys_close() {
        let mut session = Session::from_parts(opt_with_root(Some("C-F6")), example());
        assert!(matches!(session.inject("C-F6").unwrap(), Step::Show(_)));
        assert!(matches!(session.inject("Escape").unwrap(), Step::Close));

        let mut session = Session::from_parts(opt_with_root(Some("C-F6")), example());
        session.inject("C-F6").unwrap();
        assert!(matches!(session.inject("z").unwrap(), Step::Close));
    }

    #[test]
    fn titles_escape_pango_markup() {
        let mut map = HashMap::new();
        map.insert(
            "a<b".to_owned(),
            ActionDesc {
                title: "Fish & chips".to_owned(),
                action: Action::Die(()),
            },
        );
        let markup = list_markup(&map);
        assert!(markup.contains("a&lt;b"));
        assert!(markup.contains("Fish &amp; chips"));
        assert!(!markup.contains("a<b"));
    }
}
