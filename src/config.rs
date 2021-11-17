use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::action::{Action, ActionDesc, Op};
use crate::combination::KeyCombination;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub map: HashMap<KeyCombination, ActionDesc>,
}

pub fn example() -> Config {
    Config {
        map: {
            let mut m = HashMap::new();

            m.insert(
                "C-F6".to_owned(),
                ActionDesc {
                    title: "Main actions".to_owned(),
                    action: Action::Map({
                        let mut m = HashMap::new();

                        m.insert(
                            "r".to_owned(),
                            ActionDesc {
                                title: "Reload".to_owned(),
                                action: Action::Reload(()),
                            },
                        );
                        m.insert(
                            "x".to_owned(),
                            ActionDesc {
                                title: "Sub actions".to_owned(),
                                action: Action::Map({
                                    let mut m = HashMap::new();

                                    m.insert(
                                        "a".to_owned(),
                                        ActionDesc {
                                            title: "Reload".to_owned(),
                                            action: Action::Reload(()),
                                        },
                                    );
                                    m.insert(
                                        "b".to_owned(),
                                        ActionDesc {
                                            title: "Open alacritty".to_owned(),
                                            action: Action::List(vec![
                                                Op::Execute("alacritty".to_string()),
                                                Op::Reload(()),
                                            ]),
                                        },
                                    );
                                    m.insert(
                                        "c".to_owned(),
                                        ActionDesc {
                                            title: "Open file manager".to_owned(),
                                            action: Action::Execute(
                                                "exo-open --launch FileManager".to_owned(),
                                            ),
                                        },
                                    );

                                    m
                                }),
                            },
                        );
                        m.insert(
                            "e".to_owned(),
                            ActionDesc {
                                title: "Open file manager".to_owned(),
                                action: Action::Execute("exo-open --launch FileManager".to_owned()),
                            },
                        );

                        m
                    }),
                },
            );
            m.insert(
                "F11".to_owned(),
                ActionDesc {
                    title: "".to_owned(),
                    action: Action::Execute("exo-open --launch FileManager".to_owned()),
                },
            );
            m.insert(
                "C-c".to_owned(),
                ActionDesc {
                    title: "".to_owned(),
                    action: Action::Die(()),
                },
            );

            m
        },
    }
}
