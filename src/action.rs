use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type ShellScript = String;

use crate::combination::KeyCombination;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ActionDesc {
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Execute(ShellScript),
    Reload(()),
    Die(()),

    List(Vec<Op>),
    Map(HashMap<KeyCombination, ActionDesc>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Execute(ShellScript),
    Reload(()),
    Die(()),
}

impl Action {
    pub fn action_map(&self) -> Option<&HashMap<KeyCombination, ActionDesc>> {
        match self {
            Action::Map(m) => return Some(&m),
            _ => return None,
        }
    }

    pub fn to_op_list(&self) -> Vec<Op> {
        let mut v = vec![];

        match self {
            Action::Execute(e) => v.push(Op::Execute(e.clone())),
            Action::Reload(r) => v.push(Op::Reload(r.clone())),
            Action::Die(d) => v.push(Op::Die(d.clone())),
            Action::List(l) => v = l.clone(),
            Action::Map(_) => {}
        }

        v
    }
}
