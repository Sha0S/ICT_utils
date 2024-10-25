#![allow(non_snake_case)]

use std::{fs, io::Write};
use pwhash::bcrypt;

static USER_LIST: &str = "users";

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
pub enum UserLevel {
    Admin = 2,
    Engineer = 1,
    Technician = 0,
}

impl UserLevel {
    fn pepper(&self, pass: &str) -> String {
        match self {
            UserLevel::Admin => pass.to_string() + "_admin",
            UserLevel::Engineer => pass.to_string() + "_eng",
            UserLevel::Technician => pass.to_string() + "_tech",
        }
    }
}

impl From<&str> for UserLevel {
    fn from(value: &str) -> Self {
        match value {
            "2" => UserLevel::Admin,
            "1" => UserLevel::Engineer,
            _ => UserLevel::Technician,
        }
    }
}

impl UserLevel {
    fn print(&self) -> String {
        match self {
            UserLevel::Admin => String::from("2"),
            UserLevel::Engineer => String::from("1"),
            UserLevel::Technician => String::from("0"),
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
    pub fn new(name: String, level: UserLevel) -> Self {
        Self {
            name,
            level,
            hash: String::new(),
        }
    }

    pub fn create_hash(&mut self, pass: &str) {
        self.hash = bcrypt::hash(self.level.pepper(pass)).unwrap();
    }

    pub fn check_pw(&self, pass: &str) -> bool {
        bcrypt::verify(self.level.pepper(pass), &self.hash)
    }
}

pub fn load_user_list() -> Vec<User> {
    let mut ret = Vec::new();

    if let Ok(fileb) = fs::read_to_string(USER_LIST) {
        let lines: Vec<String> = fileb
            .lines()
            .filter(|f| !f.starts_with('!') && !f.is_empty())
            .map(|f| f.to_owned())
            .collect();

        for line in lines {
            let tokens: Vec<&str> = line.split('|').collect();
            if tokens.len() >= 3 {
                ret.push(User {
                    name: tokens[0].to_string(),
                    level: tokens[1].into(),
                    hash: tokens[2].to_string(),
                })
            }
        }
    }

    ret
}

pub fn save_user_list(users: &[User]) {
    if let Ok(mut file) = fs::File::create(USER_LIST) {
        for user in users {
            file.write_all(format!("{}|{}|{}\n", user.name, user.level.print(), user.hash).as_bytes()).unwrap();
        }
    }
}