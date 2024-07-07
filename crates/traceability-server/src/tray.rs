use std::sync::mpsc::SyncSender;

use tray_item::{IconSource, TrayItem};

use crate::{AppMode, UserLevel};

pub enum IconCollor {
    Green,
    Yellow,
    Red,
    Grey,
    Purple
}
pub enum Message {
    Quit,
    Settings,

    LogIn,
    LogOut,

    SetMode(AppMode),
    SetIcon(IconCollor)
}

pub fn init_tray(tx: SyncSender<Message>) -> (TrayItem, Vec<u32>) {
    let mut ret = Vec::new();

    let mut tray =
        TrayItem::new("ICT Traceability Server", IconSource::Resource("red-icon")).unwrap();

    ret.push( // 0
        tray.inner_mut()
            .add_label_with_id("ICT Traceability Server")
            .unwrap(),
    );

    tray.inner_mut().add_separator().unwrap();

    let tx_clone = tx.clone();
    ret.push( // 1
        tray.inner_mut()
            .add_menu_item_with_id("Login", move || {
                tx_clone.send(Message::LogIn).unwrap();
            })
            .unwrap(),
    );

    let tx_clone = tx.clone();
    ret.push( // 2
        tray.inner_mut()
            .add_menu_item_with_id("", move || {
                tx_clone.send(Message::SetMode(AppMode::Enabled)).unwrap();
            })
            .unwrap(),
    );

    let tx_clone = tx.clone();
    ret.push( // 3
        tray.inner_mut()
            .add_menu_item_with_id("", move || {
                tx_clone.send(Message::SetMode(AppMode::OffLine)).unwrap();
            })
            .unwrap(),
    );

    let tx_clone = tx.clone();
    ret.push( // 4
        tray.inner_mut()
            .add_menu_item_with_id("", move || {
                tx_clone.send(Message::SetMode(AppMode::Override)).unwrap();
            })
            .unwrap(),
    );

    let tx_clone = tx.clone();
    ret.push( // 5
        tray.inner_mut()
            .add_menu_item_with_id("", move || {
                tx_clone.send(Message::LogOut).unwrap();
            })
            .unwrap(),
    );

    tray.inner_mut().add_separator().unwrap();

    let settings_tx = tx.clone();
    tray.add_menu_item("Settings", move || {
        settings_tx.send(Message::Settings).unwrap();
    })
    .unwrap();

    let quit_tx = tx.clone();
    tray.add_menu_item("Quit", move || {
        quit_tx.send(Message::Quit).unwrap();
    })
    .unwrap();

    (tray, ret)
}

pub fn update_tray_login(tray: &mut TrayItem, tray_ids: &[u32], level: UserLevel) {
    tray.inner_mut().set_menu_item_label("Enable MES", tray_ids[2]).unwrap();
    tray.inner_mut().set_menu_item_label("Go Offline", tray_ids[3]).unwrap();

    if level > UserLevel::Technician {
        tray.inner_mut().set_menu_item_label("Override MES", tray_ids[4]).unwrap();
    }

    tray.inner_mut().set_menu_item_label("Logout", tray_ids[5]).unwrap();
}

pub fn update_tray_logout(tray: &mut TrayItem, tray_ids: &[u32]) {
    tray.inner_mut().set_label("ICT Traceability Server", tray_ids[0]).unwrap();
    tray.inner_mut().set_menu_item_label("", tray_ids[2]).unwrap();
    tray.inner_mut().set_menu_item_label("", tray_ids[3]).unwrap();
    tray.inner_mut().set_menu_item_label("", tray_ids[4]).unwrap();
    tray.inner_mut().set_menu_item_label("", tray_ids[5]).unwrap();
}