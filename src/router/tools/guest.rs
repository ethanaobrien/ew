use jzon::{array, object, JsonValue};
use crate::router::userdata;
use crate::router::global;

fn get_clear_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for current in user["live_list"].members() {
        if current["level"] == level {
            rv += 1;
        }
    }
    rv
}

fn get_full_combo_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for current in user["live_mission_list"].members() {
        if current["clear_master_live_mission_ids"].contains(20 + level) {
            rv += 1;
        }
    }
    rv
}

fn get_perfect_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for current in user["live_mission_list"].members() {
        if current["clear_master_live_mission_ids"].contains(40 + level) {
            rv += 1;
        }
    }
    rv
}

fn get_high_score_rate(user: &JsonValue) -> JsonValue {
    let mut entries = vec![];
    for live in user["live_list"].members() {
        let rate = live["high_score"].as_i64().unwrap_or(0) / 5000;
        entries.push((rate, live["master_live_id"].as_i64().unwrap_or(0), live["level"].as_i64().unwrap_or(0)));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    entries.truncate(10);

    let mut detail = array![];
    let mut total = 0;
    for (rate, master_live_id, level) in entries {
        total += rate;
        detail.push(object!{
            master_live_id: master_live_id,
            level: level,
            rate: rate
        }).unwrap();
    }

    object!{
        rate: total,
        detail: detail
    }
}

#[derive(Clone, Copy)]
pub enum UserView {
    Card,
    Detail,
    Ranking,
}

pub fn get_user(id: i64, friends: &JsonValue, view: UserView) -> JsonValue {
    let user = userdata::get_acc_from_uid(id);
    if !user["error"].is_empty() {
        return object!{};
    }

    let mut rv = object!{
        user: user["user"].clone(),
        favorite_card: global::get_card(user["user"]["favorite_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_smile_card: global::get_card(user["user"]["guest_smile_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_cool_card: global::get_card(user["user"]["guest_cool_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_pure_card: global::get_card(user["user"]["guest_pure_master_card_id"].as_i64().unwrap_or(0), &user)
    };

    if let UserView::Detail | UserView::Ranking = view {
        rv["main_deck_detail"] = object!{
            total_power: 0,
            deck: user["deck_list"][user["user"]["main_deck_slot"].as_usize().unwrap_or(1) - 1].clone(),
            card_list: global::get_cards(user["deck_list"][user["user"]["main_deck_slot"].as_usize().unwrap_or(1) - 1]["main_card_ids"].clone(), &user)
        };
        rv["master_title_ids"] = user["user"]["master_title_ids"].clone();
    }

    if let UserView::Detail = view {
        rv["live_data_summary"] = object!{
            clear_count_list: [get_clear_count(&user, 1), get_clear_count(&user, 2), get_clear_count(&user, 3), get_clear_count(&user, 4)],
            full_combo_list: [get_full_combo_count(&user, 1), get_full_combo_count(&user, 2), get_full_combo_count(&user, 3), get_full_combo_count(&user, 4)],
            all_perfect_list: [get_perfect_count(&user, 1), get_perfect_count(&user, 2), get_perfect_count(&user, 3), get_perfect_count(&user, 4)],
            high_score_rate: get_high_score_rate(&user)
        };
    }

    rv["user"].remove("sif_user_id");
    rv["user"].remove("ss_user_id");
    rv["user"].remove("birthday");

    if let UserView::Card | UserView::Ranking = view {
        if !friends.is_empty() {
            rv["status"] = if friends["friend_user_id_list"].contains(id) {
                3
            } else if friends["pending_user_id_list"].contains(id) {
                2
            } else if friends["request_user_id_list"].contains(id) {
                1
            } else {
                0
            }.into();
        }
    }

    rv
}
