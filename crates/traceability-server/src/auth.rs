#![allow(dead_code)]
use std::fs;

use pwhash::bcrypt;

static USER_LIST: &str = "users";

#[derive(Debug, Clone, PartialEq)]
pub enum UserLevel {
    Admin,
    Engineer,
    Tech,
}

impl UserLevel {
    fn pepper(&self, pass: String) -> String {
        match self {
            UserLevel::Admin => pass + "_admin",
            UserLevel::Engineer => pass + "_eng",
            UserLevel::Tech => pass + "_tech",
        }
    }
}

impl From<&str> for UserLevel {
    fn from(value: &str) -> Self {
        match value {
            "0" => UserLevel::Admin,
            "1" => UserLevel::Engineer,
            _ => UserLevel::Tech,
        }
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub name: String,
    pub level: UserLevel,
    hash: String,
}

impl User {
    fn new(name: String, level: UserLevel) -> Self {
        Self {
            name,
            level,
            hash: String::new(),
        }
    }

    fn create_hash(&mut self, pass: String) {
        self.hash = bcrypt::hash(self.level.pepper(pass)).unwrap();
    }

    pub fn check_pw(&self, pass: String) -> bool {
        bcrypt::verify(self.level.pepper(pass), &self.hash)
    }
}

pub fn load_user_list() -> Vec<User> {
    let mut ret = Vec::new();

    if let Ok(fileb) = fs::read_to_string(USER_LIST) {
        let lines: Vec<String> = fileb
            .lines()
            .filter(|f| !f.starts_with('!'))
            .map(|f| f.to_owned())
            .collect();

        for line in lines {
            let tokens: Vec<&str> = line.split('|').collect();
            ret.push(User {
                name: tokens[0].to_string(),
                level: tokens[1].into(),
                hash: tokens[2].to_string(),
            })
        }
    }

    ret
}
