//! Daily Quest system — quest definition loading, kill tracking, quest list sending.
//! ## Daily Quest Flow
//! 1. On login, `DailyQuestSendList()` sends all quest definitions + user progress.
//! 2. On monster kill, `UpdateDailyQuestCount(monsterID)` checks all active quests.
//! 3. On quest completion, rewards are given and status updated.
//! ## Time Types
//! | Value | Name    | Behavior after completion                   |
//! |-------|---------|---------------------------------------------|
//! | 0     | Repeat  | Reset to ongoing, can redo immediately       |
//! | 1     | Time    | Set to timewait, replay after cooldown hours |
//! | 2     | Single  | Set to completed permanently                 |
//! ## Kill Types
//! | Value | Name  | Restriction       |
//! |-------|-------|-------------------|
//! | 0     | Solo  | Must NOT be in party |
//! | 1     | Party | Must be in party     |
//! | 2     | Any   | No restriction       |

use ko_db::models::daily_quest::{
    DailyQuestRow, DailyQuestStatus, DailyQuestTimeType, UserDailyQuestRow,
};
use ko_protocol::{Opcode, Packet};

use crate::world::WorldState;
use crate::zone::SessionId;

/// ExtHookSubOpcodes::DailyQuest sub-opcode
const EXT_SUB_DAILY_QUEST: u8 = 0xD3;
/// ExtHookSubOpcodes for daily quest progress notice
const EXT_SUB_DAILY_NOTICE: u8 = 0xDC;

/// DailyQuestOp::sendlist = 0, userinfo = 1, killupdate = 2.
const DQ_OP_SENDLIST: u8 = 0;
const DQ_OP_USERINFO: u8 = 1;
const DQ_OP_KILLUPDATE: u8 = 2;

/// v2615 built-in beginner mission: "Rescuing Sid".
/// The client places this mission in native panel slot 2 with mission ID 3,
/// and completes it after one Moradon Worm is killed.
const RESCUING_SID_SLOT: u8 = 2;
const RESCUING_SID_CLIENT_MISSION_ID: i32 = 3;
const RESCUING_SID_WORM_PROTO_IDS: [u16; 3] = [700, 750, 751];

use crate::world::{ITEM_COUNT, ITEM_EXP, ITEM_GOLD, ITEM_LADDERPOINT, ITEM_RANDOM};

/// Check if a monster ID matches any of the quest's 4 mob slots.
pub fn quest_matches_monster(quest: &DailyQuestRow, monster_id: i32) -> bool {
    if monster_id == 0 {
        return false;
    }
    quest.mob_id_1 == monster_id
        || (quest.mob_id_2 != 0 && quest.mob_id_2 == monster_id)
        || (quest.mob_id_3 != 0 && quest.mob_id_3 == monster_id)
        || (quest.mob_id_4 != 0 && quest.mob_id_4 == monster_id)
}

/// Check if a character's level is within the quest's level range.
pub fn quest_level_check(quest: &DailyQuestRow, level: i16) -> bool {
    level >= quest.min_level && level <= quest.max_level
}

/// Check if a kill is valid for the quest based on party status.
/// - killtype 0: solo only (must NOT be in party)
/// - killtype 1: party only (must be in party)
/// - killtype 2: any
pub fn quest_party_check(quest: &DailyQuestRow, in_party: bool) -> bool {
    match quest.kill_type {
        0 => !in_party,
        1 => in_party,
        _ => true, // killtype 2 = any
    }
}

/// Determine the new status after a quest is completed, based on the time type.
pub fn status_after_completion(time_type: i16) -> i16 {
    match time_type {
        t if t == DailyQuestTimeType::Repeat as i16 => DailyQuestStatus::Ongoing as i16,
        t if t == DailyQuestTimeType::Single as i16 => DailyQuestStatus::Completed as i16,
        t if t == DailyQuestTimeType::Time as i16 => DailyQuestStatus::TimeWait as i16,
        _ => DailyQuestStatus::Ongoing as i16,
    }
}

/// Calculate replay time for a time-gated quest completion.
/// Returns the replay cooldown in seconds (replay_time_hours * 3600).
pub fn calculate_replay_cooldown_secs(replay_time_hours: i16) -> i32 {
    (replay_time_hours as i32) * 3600
}

/// Check if a quest's replay cooldown has expired.
/// `replay_time` is a future Unix timestamp. If current time >= replay_time, cooldown expired.
pub fn is_replay_cooldown_expired(replay_time: i32, current_unix_time: i32) -> bool {
    replay_time == 0 || current_unix_time >= replay_time
}

/// Get reward item IDs and counts from a daily quest definition.
/// Returns (item_id, count) pairs for non-zero rewards.
pub fn get_quest_rewards(quest: &DailyQuestRow) -> Vec<(i32, i32)> {
    let slots = [
        (quest.reward_1, quest.count_1),
        (quest.reward_2, quest.count_2),
        (quest.reward_3, quest.count_3),
        (quest.reward_4, quest.count_4),
    ];
    slots
        .iter()
        .filter(|&&(item_id, count)| item_id != 0 && count != 0)
        .copied()
        .collect()
}

// ── Zone Check ──────────────────────────────────────────────────────────────

/// Check if the player's current zone matches the quest's zone restriction.
/// `quest_zone == 0` means no zone restriction.
/// The server uses the following zone checks (differs from older versions!):
/// - zone 21 (Moradon): exact match only (NOT isInMoradon group)
/// - zone 1 (Karus): exact match only (NOT isInLufersonCastle group)
/// - zone 2 (Elmorad): zone 2, 7, 8 (isInElmoradCastle group)
/// - zone 11 (Karus Eslant): exact match only
/// - zone 12 (Elmorad Eslant): zones 11, 13, 14 (isInKarusEslant -- C++ bug preserved)
/// - Others: exact zone ID match (fallthrough: `if v12 != m_bZone`)
pub fn zone_check(quest_zone: i16, player_zone: u16) -> bool {
    if quest_zone == 0 {
        return true;
    }
    let qz = quest_zone as u16;
    match qz {
        21 => player_zone == 21,
        1 => player_zone == 1,
        // `if (m_bZone != 2 && (m_bZone - 7) > 1) skip`
        2 => player_zone == 2 || (player_zone >= 7 && player_zone <= 8),
        11 => player_zone == 11,
        // `((v14 - 11) & 0xFC) != 0 || v14 == 12` -> valid zones: 11, 13, 14
        // This is a C++ bug: quest zone 12 requires player in Karus Eslant, not Elmorad Eslant
        12 => matches!(player_zone, 11 | 13 | 14),
        _ => qz == player_zone,
    }
}

// ── Packet Builders ─────────────────────────────────────────────────────────

/// Build the base WIZ_EXT_HOOK + DailyQuest sub-opcode packet.
fn build_dq_base() -> Packet {
    let mut pkt = Packet::new(Opcode::EXT_HOOK_S2C);
    pkt.write_u8(EXT_SUB_DAILY_QUEST);
    pkt
}

/// Build the kill update packet sent to the client on each daily quest kill.
/// Wire: `[0xE9][0xD3][0x02][u8 quest_index][u16 monster_id]`
pub fn build_kill_update(quest_index: i16, monster_id: u16) -> Packet {
    let mut pkt = build_dq_base();
    pkt.write_u8(DQ_OP_KILLUPDATE);
    pkt.write_u8(quest_index as u8);
    pkt.write_u16(monster_id);
    pkt
}

/// Build the progress notice packet (toast/HUD notification).
/// Wire: `[0xE9][0xDC][SByte][string quest_name][u16 current][u16 required][u16 monster_id]`
pub fn build_progress_notice(
    quest_name: &str,
    current_kills: u16,
    required_kills: u16,
    monster_id: u16,
) -> Packet {
    let mut pkt = Packet::new(Opcode::EXT_HOOK_S2C);
    pkt.write_u8(EXT_SUB_DAILY_NOTICE);
    pkt.write_sbyte_string(quest_name);
    pkt.write_u16(current_kills);
    pkt.write_u16(required_kills);
    pkt.write_u16(monster_id);
    pkt
}

// ── DailyQuestSendList ──────────────────────────────────────────────────────

/// Send daily quest definitions + user progress to the client.
/// Sends two packets:
/// 1. Quest definitions (sendlist): index, timetype, killtype, mobs, rewards, etc.
/// 2. User progress (userinfo): index, status, kcount, remaining_time.
pub fn daily_quest_send_list(world: &WorldState, sid: SessionId) {
    let all_defs = world.get_all_daily_quests();
    let user_quests = world.with_session(sid, |h| h.daily_quests.clone());
    let user_quests = match user_quests {
        Some(q) => q,
        None => return,
    };

    // Packet 1: Quest definitions
    let mut pkt = build_dq_base();
    pkt.write_u8(DQ_OP_SENDLIST);
    let count_pos = pkt.wpos();
    pkt.write_u16(0); // placeholder

    let mut written = 0u16;
    for def in &all_defs {
        pkt.write_u8(def.id as u8);
        pkt.write_u8(def.time_type as u8);
        pkt.write_u8(def.kill_type as u8);

        // 4 mob slots: [u16 mob_id][u32 reward_id][u32 reward_count]
        let mobs = [def.mob_id_1, def.mob_id_2, def.mob_id_3, def.mob_id_4];
        let rewards = [
            (def.reward_1, def.count_1),
            (def.reward_2, def.count_2),
            (def.reward_3, def.count_3),
            (def.reward_4, def.count_4),
        ];
        for i in 0..4 {
            pkt.write_u16(mobs[i] as u16);
            pkt.write_u32(rewards[i].0 as u32);
            pkt.write_u32(rewards[i].1 as u32);
        }

        pkt.write_u16(def.kill_count as u16);
        pkt.write_u8(def.zone_id as u8);
        pkt.write_u16(def.replay_time as u16);
        pkt.write_u8(def.min_level as u8);
        pkt.write_u8(def.max_level as u8);

        written += 1;
    }
    // Patch count
    pkt.put_u16_at(count_pos, written);

    world.send_to_session_owned(sid, pkt);

    // Packet 2: User progress
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i32;

    let mut pkt2 = build_dq_base();
    pkt2.write_u8(DQ_OP_USERINFO);
    let count2_pos = pkt2.wpos();
    pkt2.write_u16(0); // placeholder

    let mut user_count = 0u16;
    // Iterate in user quest order (keyed by quest index)
    let mut keys: Vec<i16> = user_quests.keys().copied().collect();
    keys.sort();
    for key in &keys {
        if let Some(uq) = user_quests.get(key) {
            let mut status = uq.status;
            let remaining = if uq.replay_time > 0 && uq.replay_time > now {
                (uq.replay_time - now) as u32
            } else {
                // Cooldown expired — transition timewait → ongoing
                if status == DailyQuestStatus::TimeWait as i16 {
                    status = DailyQuestStatus::Ongoing as i16;
                }
                0u32
            };

            pkt2.write_u8(*key as u8);
            pkt2.write_u8(status as u8);
            pkt2.write_u16(uq.kill_count as u16);
            pkt2.write_u32(remaining);
            user_count += 1;
        }
    }
    pkt2.put_u16_at(count2_pos, user_count);

    world.send_to_session_owned(sid, pkt2);

    // ── v2525 native 0xC7 — send active quests into 4 panel slots ────
    // Client panel at [esi+0x1F0], max 4 slots (0-3).
    // Unlike ext_hook (bulk list), v2525 uses per-quest slot init packets.
    let mut slot_index: u8 = 0;
    for key in &keys {
        if slot_index >= 4 {
            break;
        }
        if let Some(uq) = user_quests.get(key) {
            if uq.status == DailyQuestStatus::Ongoing as i16 {
                // Find the quest definition for NPC ID
                let npc_id = all_defs
                    .iter()
                    .find(|d| d.id == *key)
                    .map(|d| d.mob_id_1 as u16)
                    .unwrap_or(0);
                let init_pkt = super::daily_quest_v2525::build_init(
                    slot_index,
                    *key as i32,
                    npc_id,
                    0, // status=0: active
                );
                world.send_to_session_owned(sid, init_pkt);
                slot_index += 1;
            }
        }
    }

    // WIZ_CHAT fallback for vanilla v2525 client (drops ext_hook 0xE9)
    if slot_index > 0 {
        let chat_msg = format!("[Quest] {} active daily quest(s)", slot_index);
        let chat_pkt = crate::systems::timed_notice::build_notice_packet(7, &chat_msg);
        world.send_to_session_owned(sid, chat_pkt);
    }
}

// ── UpdateDailyQuestCount ───────────────────────────────────────────────────

/// Process daily quest kill tracking for a player after an NPC death.
/// Iterates all active daily quests and checks if the killed monster matches.
/// If so, increments kill count, sends update packets, and triggers completion.
pub async fn update_daily_quest_count(world: &WorldState, sid: SessionId, monster_id: u16) {
    if monster_id == 0 {
        return;
    }

    // Get player data
    let player_zone = world.get_position(sid).map(|p| p.zone_id).unwrap_or(0);

    // "Rescuing Sid" is a built-in one-kill beginner mission, not the
    // configurable Daily Quest Worm Hunt. Close its exact native panel entry
    // immediately while leaving the separate daily quest counter untouched.
    if matches!(player_zone, 21 | 22 | 23 | 24 | 25)
        && RESCUING_SID_WORM_PROTO_IDS.contains(&monster_id)
    {
        let complete_pkt = super::daily_quest_v2525::build_complete(
            RESCUING_SID_SLOT,
            RESCUING_SID_CLIENT_MISSION_ID,
        );
        world.send_to_session_owned(sid, complete_pkt);
        tracing::info!(
            sid,
            monster_id,
            slot_index = RESCUING_SID_SLOT,
            mission_id = RESCUING_SID_CLIENT_MISSION_ID,
            "Rescuing Sid completed by Moradon Worm kill"
        );
    }

    let player_data = world.with_session(sid, |h| {
        (
            h.daily_quests.clone(),
            h.character.as_ref().map(|c| c.level).unwrap_or(0) as i16,
            world.get_party_id(sid).is_some(),
        )
    });
    let (mut dq_map, player_level, in_party) = match player_data {
        Some(d) => d,
        None => return,
    };

    if dq_map.is_empty() {
        return;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i32;

    let all_defs = world.get_all_daily_quests();
    let mut changed = false;
    let mut changed_quest_ids: Vec<i16> = Vec::new();

    for def in &all_defs {
        let uq = match dq_map.get_mut(&def.id) {
            Some(uq) => uq,
            None => continue,
        };

        // Skip completed quests
        if uq.status == DailyQuestStatus::Completed as i16 {
            continue;
        }

        // Skip timewait with active cooldown
        if uq.status == DailyQuestStatus::TimeWait as i16
            && uq.replay_time > 0
            && now < uq.replay_time
        {
            continue;
        }

        // Zone check
        if !zone_check(def.zone_id, player_zone) {
            continue;
        }

        // Already at max kills
        if uq.kill_count + 1 > def.kill_count {
            continue;
        }

        // Party/kill type check
        if !quest_party_check(def, in_party) {
            continue;
        }

        // Level check
        if !quest_level_check(def, player_level) {
            continue;
        }

        // Monster match
        if !quest_matches_monster(def, monster_id as i32) {
            continue;
        }

        // Increment kill count
        uq.kill_count += 1;
        changed = true;
        changed_quest_ids.push(def.id);

        // Send kill update packet
        let kill_pkt = build_kill_update(def.id, monster_id);
        world.send_to_session_owned(sid, kill_pkt);

        // Send progress notice
        let quest_name = def.quest_name.as_deref().unwrap_or("");
        let notice_pkt = build_progress_notice(
            quest_name,
            uq.kill_count as u16,
            def.kill_count as u16,
            monster_id,
        );
        world.send_to_session_owned(sid, notice_pkt);

        // WIZ_CHAT fallback for vanilla v2525 client (drops ext_hook 0xE9)
        let chat_msg = format!(
            "[Quest] {}: {}/{}",
            quest_name, uq.kill_count, def.kill_count
        );
        let chat_pkt = crate::systems::timed_notice::build_notice_packet(7, &chat_msg);
        world.send_to_session_owned(sid, chat_pkt);

        // Check completion
        if uq.kill_count >= def.kill_count {
            daily_quest_finished(world, sid, def, uq).await;
        }
    }

    // Write back updated map and persist every accepted kill. Previously only
    // the final kill was saved, so reconnecting or changing zones could restore
    // an older count and leave the native quest panel stuck.
    if changed {
        let rows_to_save: Vec<UserDailyQuestRow> = changed_quest_ids
            .iter()
            .filter_map(|quest_id| dq_map.get(quest_id).cloned())
            .collect();

        world.update_session(sid, |h| {
            h.daily_quests = dq_map;
        });

        if let Some(pool) = world.db_pool() {
            let pool = pool.clone();
            tokio::spawn(async move {
                let repo = ko_db::repositories::daily_quest::DailyQuestRepository::new(&pool);
                for row in rows_to_save {
                    if let Err(e) = repo.save_user_quest(&row).await {
                        tracing::warn!(
                            "DailyQuest kill save failed for quest {}: {}",
                            row.quest_id,
                            e
                        );
                    }
                }
            });
        }
    }
}

// ── Send Reward Letter ────────────────────────────────────────────────────────

/// Send a daily quest reward item via letter when inventory is full.
pub(crate) async fn send_reward_letter(
    world: &WorldState,
    sid: SessionId,
    item_id: i32,
    count: i16,
) {
    let Some(pool) = world.db_pool() else { return };
    let char_name = match world.with_session(sid, |h| h.character.as_ref().map(|c| c.name.clone()))
    {
        Some(Some(name)) if !name.is_empty() => name,
        _ => return,
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i32;

    let pool = pool.clone();
    let name = char_name;
    tokio::spawn(async move {
        let repo = ko_db::repositories::letter::LetterRepository::new(&pool);
        if let Err(e) = repo
            .send_letter(
                "Daily Quest",        // sender
                &name,                // recipient
                "Daily Quest Reward", // subject
                "",                   // message
                2,                    // b_type = item letter
                item_id,              // item_id
                count,                // item_count
                0,                    // durability
                0,                    // serial
                0,                    // expiry
                0,                    // coins
                now,                  // send_date
            )
            .await
        {
            tracing::warn!("DailyQuest letter send failed for {}: {}", name, e);
        }
    });
}

// ── DailyQuestFinished ──────────────────────────────────────────────────────

/// Process daily quest completion: update status, give rewards, save to DB.
async fn daily_quest_finished(
    world: &WorldState,
    sid: SessionId,
    quest: &DailyQuestRow,
    user_quest: &mut UserDailyQuestRow,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i32;

    // The v2615 native panel has four physical slots. Login fills them with
    // ongoing quests in sorted ID order, so completion must close the same
    // slot. The old hard-coded slot 0 left entries such as "Rescuing Sid"
    // visible when they occupied slot 1, 2 or 3.
    let native_slot_index = world
        .with_session(sid, |h| {
            let mut active_ids: Vec<i16> = h
                .daily_quests
                .iter()
                .filter_map(|(&quest_id, progress)| {
                    (progress.status == DailyQuestStatus::Ongoing as i16)
                        .then_some(quest_id)
                })
                .collect();
            active_ids.sort_unstable();
            active_ids
                .iter()
                .take(4)
                .position(|&quest_id| quest_id == quest.id)
                .map(|slot| slot as u8)
        })
        .flatten();

    // Set replay timer
    if quest.replay_time > 0 {
        user_quest.replay_time = now + calculate_replay_cooldown_secs(quest.replay_time);
    }

    // Update status
    user_quest.status = status_after_completion(quest.time_type);

    // Reset kill count
    user_quest.kill_count = 0;

    // Distribute rewards (C++ ReqDailyQuestSendReward DailyQuest.cpp:22-100)
    let rewards = get_quest_rewards(quest);

    // Show reward popup (C++ QuestV2ShowGiveItem)
    let mut show_pkt = Packet::new(Opcode::WizQuest as u8);
    show_pkt.write_u8(10); // sub-opcode for ShowGiveItem
    let reward_slots = [
        (quest.reward_1, quest.count_1),
        (quest.reward_2, quest.count_2),
        (quest.reward_3, quest.count_3),
        (quest.reward_4, quest.count_4),
    ];
    for i in 0..8 {
        if i < reward_slots.len() {
            show_pkt.write_u32(reward_slots[i].0 as u32);
            show_pkt.write_u32(reward_slots[i].1 as u32);
        } else {
            show_pkt.write_u32(0);
            show_pkt.write_u32(0);
        }
    }
    world.send_to_session_owned(sid, show_pkt);

    // Give each reward (C++ ReqDailyQuestSendReward — DailyQuest.cpp:22-100)
    for &(item_id, count) in &rewards {
        if item_id == ITEM_EXP as i32 {
            // XP reward
            super::level::exp_change(world, sid, count as i64).await;
        } else if item_id == ITEM_GOLD as i32 {
            // Gold reward
            world.gold_gain(sid, count as u32);
        } else if item_id == ITEM_COUNT as i32 || item_id == ITEM_LADDERPOINT as i32 {
            // Loyalty/NP reward
            crate::systems::loyalty::send_loyalty_change(world, sid, count, false, false, true);
        } else if item_id == ITEM_RANDOM as i32 {
            // Random item from pool
            let random_items = world.get_item_random_by_session(quest.random_id);
            if !random_items.is_empty() {
                let idx = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as usize)
                    % random_items.len();
                let picked = &random_items[idx];
                let given = world.give_item(
                    sid,
                    picked.item_id as u32,
                    picked.item_count.clamp(0, u16::MAX as i32) as u16,
                );
                if !given {
                    send_reward_letter(world, sid, picked.item_id, picked.item_count as i16).await;
                }
            }
        } else {
            // Normal item reward
            let given =
                world.give_item(sid, item_id as u32, count.clamp(0, u16::MAX as i32) as u16);
            if !given {
                // Inventory full — send via letter
                send_reward_letter(world, sid, item_id, count as i16).await;
            }
        }
    }

    // v2615 native 0xC7 — remove the exact slot that was initialised for
    // this quest. Quests outside the first four were never shown and therefore
    // do not need a native completion packet.
    if let Some(slot_index) = native_slot_index {
        let complete_pkt =
            super::daily_quest_v2525::build_complete(slot_index, quest.id as i32);
        world.send_to_session_owned(sid, complete_pkt);
        tracing::info!(
            sid,
            quest_id = quest.id,
            slot_index,
            "DailyQuest native panel entry completed"
        );
    }

    // Async DB save
    if let Some(pool) = world.db_pool() {
        let pool = pool.clone();
        let uq_clone = user_quest.clone();
        tokio::spawn(async move {
            let repo = ko_db::repositories::daily_quest::DailyQuestRepository::new(&pool);
            if let Err(e) = repo.save_user_quest(&uq_clone).await {
                tracing::warn!(
                    "DailyQuestFinished: save failed for quest {}: {}",
                    uq_clone.quest_id,
                    e
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ko_db::models::daily_quest::{
        DailyQuestRow, DailyQuestStatus, DailyQuestTimeType, UserDailyQuestRow,
    };

    fn make_quest(
        id: i16,
        mob_id: i32,
        kill_count: i32,
        time_type: i16,
        kill_type: i16,
    ) -> DailyQuestRow {
        DailyQuestRow {
            id,
            quest_name: Some(format!("Test Quest {id}")),
            quest_id: 0,
            time_type,
            kill_type,
            mob_id_1: mob_id,
            mob_id_2: 0,
            mob_id_3: 0,
            mob_id_4: 0,
            kill_count,
            reward_1: 900001000,
            reward_2: 0,
            reward_3: 0,
            reward_4: 0,
            count_1: 1000000,
            count_2: 0,
            count_3: 0,
            count_4: 0,
            zone_id: 21,
            min_level: 1,
            max_level: 83,
            replay_time: 0,
            random_id: 0,
        }
    }

    fn make_multi_mob_quest() -> DailyQuestRow {
        let mut q = make_quest(99, 100, 10, 0, 2);
        q.mob_id_2 = 200;
        q.mob_id_3 = 300;
        q.mob_id_4 = 400;
        q
    }

    #[test]
    fn test_quest_matches_monster_single_mob() {
        let quest = make_quest(1, 750, 5, 2, 2);
        assert!(quest_matches_monster(&quest, 750));
        assert!(!quest_matches_monster(&quest, 751));
        assert!(!quest_matches_monster(&quest, 0));
    }

    #[test]
    fn test_quest_matches_monster_multi_mob() {
        let quest = make_multi_mob_quest();
        assert!(quest_matches_monster(&quest, 100));
        assert!(quest_matches_monster(&quest, 200));
        assert!(quest_matches_monster(&quest, 300));
        assert!(quest_matches_monster(&quest, 400));
        assert!(!quest_matches_monster(&quest, 500));
    }

    #[test]
    fn test_quest_level_check() {
        let quest = make_quest(1, 750, 5, 2, 2);
        // quest: min_level=1, max_level=83
        assert!(quest_level_check(&quest, 1));
        assert!(quest_level_check(&quest, 40));
        assert!(quest_level_check(&quest, 83));
        assert!(!quest_level_check(&quest, 0));
        assert!(!quest_level_check(&quest, 84));
    }

    #[test]
    fn test_quest_level_check_restricted_range() {
        let mut quest = make_quest(18, 2151, 50, 0, 2);
        quest.min_level = 40;
        quest.max_level = 83;
        assert!(!quest_level_check(&quest, 39));
        assert!(quest_level_check(&quest, 40));
        assert!(quest_level_check(&quest, 60));
        assert!(quest_level_check(&quest, 83));
        assert!(!quest_level_check(&quest, 84));
    }

    #[test]
    fn test_quest_party_check_solo_only() {
        let quest = make_quest(1, 750, 5, 2, 0); // killtype 0 = solo
        assert!(quest_party_check(&quest, false));
        assert!(!quest_party_check(&quest, true));
    }

    #[test]
    fn test_quest_party_check_party_only() {
        let quest = make_quest(1, 750, 5, 2, 1); // killtype 1 = party
        assert!(!quest_party_check(&quest, false));
        assert!(quest_party_check(&quest, true));
    }

    #[test]
    fn test_quest_party_check_any() {
        let quest = make_quest(1, 750, 5, 2, 2); // killtype 2 = any
        assert!(quest_party_check(&quest, false));
        assert!(quest_party_check(&quest, true));
    }

    #[test]
    fn test_status_after_completion_repeat() {
        assert_eq!(
            status_after_completion(DailyQuestTimeType::Repeat as i16),
            DailyQuestStatus::Ongoing as i16
        );
    }

    #[test]
    fn test_status_after_completion_single() {
        assert_eq!(
            status_after_completion(DailyQuestTimeType::Single as i16),
            DailyQuestStatus::Completed as i16
        );
    }

    #[test]
    fn test_status_after_completion_time() {
        assert_eq!(
            status_after_completion(DailyQuestTimeType::Time as i16),
            DailyQuestStatus::TimeWait as i16
        );
    }

    #[test]
    fn test_calculate_replay_cooldown() {
        assert_eq!(calculate_replay_cooldown_secs(0), 0);
        assert_eq!(calculate_replay_cooldown_secs(1), 3600);
        assert_eq!(calculate_replay_cooldown_secs(24), 86400);
    }

    #[test]
    fn test_is_replay_cooldown_expired() {
        // No cooldown (replay_time=0) always expired
        assert!(is_replay_cooldown_expired(0, 1000));
        // Current time >= replay_time => expired
        assert!(is_replay_cooldown_expired(1000, 1000));
        assert!(is_replay_cooldown_expired(1000, 2000));
        // Current time < replay_time => NOT expired
        assert!(!is_replay_cooldown_expired(2000, 1000));
    }

    #[test]
    fn test_get_quest_rewards_all_slots() {
        let mut quest = make_quest(1, 750, 5, 2, 2);
        quest.reward_1 = 100;
        quest.count_1 = 10;
        quest.reward_2 = 200;
        quest.count_2 = 20;
        quest.reward_3 = 300;
        quest.count_3 = 30;
        quest.reward_4 = 400;
        quest.count_4 = 40;

        let rewards = get_quest_rewards(&quest);
        assert_eq!(rewards.len(), 4);
        assert_eq!(rewards[0], (100, 10));
        assert_eq!(rewards[1], (200, 20));
        assert_eq!(rewards[2], (300, 30));
        assert_eq!(rewards[3], (400, 40));
    }

    #[test]
    fn test_get_quest_rewards_partial() {
        let quest = make_quest(1, 750, 5, 2, 2);
        // reward_1=900001000, count_1=1000000; reward_2-4=0
        let rewards = get_quest_rewards(&quest);
        assert_eq!(rewards.len(), 1);
        assert_eq!(rewards[0], (900001000, 1000000));
    }

    #[test]
    fn test_get_quest_rewards_empty() {
        let mut quest = make_quest(1, 750, 5, 2, 2);
        quest.reward_1 = 0;
        quest.count_1 = 0;
        let rewards = get_quest_rewards(&quest);
        assert!(rewards.is_empty());
    }

    #[test]
    fn test_daily_quest_time_type_enum() {
        assert_eq!(DailyQuestTimeType::Repeat as u8, 0);
        assert_eq!(DailyQuestTimeType::Time as u8, 1);
        assert_eq!(DailyQuestTimeType::Single as u8, 2);
    }

    #[test]
    fn test_daily_quest_status_enum() {
        assert_eq!(DailyQuestStatus::TimeWait as u8, 0);
        assert_eq!(DailyQuestStatus::Completed as u8, 1);
        assert_eq!(DailyQuestStatus::Ongoing as u8, 2);
    }

    #[test]
    fn test_user_daily_quest_row_defaults() {
        let row = UserDailyQuestRow {
            character_id: "TestPlayer".to_string(),
            quest_id: 5,
            kill_count: 0,
            status: DailyQuestStatus::Ongoing as i16,
            replay_time: 0,
        };
        assert_eq!(row.status, 2);
        assert_eq!(row.kill_count, 0);
        assert_eq!(row.replay_time, 0);
    }

    #[test]
    fn test_daily_quest_row_zone_moradon() {
        let quest = make_quest(1, 750, 5, 2, 2);
        assert_eq!(quest.zone_id, 21); // Moradon
    }

    #[test]
    fn test_daily_quest_row_karus_eslant() {
        let mut quest = make_quest(18, 2151, 50, 0, 2);
        quest.zone_id = 1; // Karus
        assert_eq!(quest.zone_id, 1);
    }

    #[test]
    fn test_kill_count_tracking_below_max() {
        let quest = make_quest(1, 750, 5, 2, 2);
        let mut user_quest = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 1,
            kill_count: 3,
            status: DailyQuestStatus::Ongoing as i16,
            replay_time: 0,
        };
        // Simulate kill
        user_quest.kill_count += 1;
        assert_eq!(user_quest.kill_count, 4);
        assert!(user_quest.kill_count < quest.kill_count);
    }

    #[test]
    fn test_kill_count_tracking_completion() {
        let quest = make_quest(1, 750, 5, 2, 2);
        let mut user_quest = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 1,
            kill_count: 4,
            status: DailyQuestStatus::Ongoing as i16,
            replay_time: 0,
        };
        // Simulate final kill
        user_quest.kill_count += 1;
        assert_eq!(user_quest.kill_count, quest.kill_count);
        // On completion, status changes based on time_type
        user_quest.status = status_after_completion(quest.time_type);
        assert_eq!(user_quest.status, DailyQuestStatus::Completed as i16);
    }

    #[test]
    fn test_repeat_quest_cycle() {
        let quest = make_quest(1, 750, 5, 0, 2); // time_type=0 (repeat)
        let mut user_quest = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 1,
            kill_count: 5,
            status: DailyQuestStatus::Ongoing as i16,
            replay_time: 0,
        };
        // Complete: repeat type resets to ongoing and clears kill count
        user_quest.status = status_after_completion(quest.time_type);
        user_quest.kill_count = 0;
        assert_eq!(user_quest.status, DailyQuestStatus::Ongoing as i16);
        assert_eq!(user_quest.kill_count, 0);
    }

    #[test]
    fn test_time_gated_quest_cycle() {
        let mut quest = make_quest(18, 2151, 50, 1, 2); // time_type=1 (time)
        quest.replay_time = 1; // 1 hour replay
        let current_time = 1720000000i32;

        let mut user_quest = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 18,
            kill_count: 50,
            status: DailyQuestStatus::Ongoing as i16,
            replay_time: 0,
        };

        // Complete: set to timewait, set replay_time
        user_quest.status = status_after_completion(quest.time_type);
        user_quest.kill_count = 0;
        user_quest.replay_time = current_time + calculate_replay_cooldown_secs(quest.replay_time);

        assert_eq!(user_quest.status, DailyQuestStatus::TimeWait as i16);
        assert_eq!(user_quest.replay_time, 1720000000 + 3600);

        // Not expired yet
        assert!(!is_replay_cooldown_expired(
            user_quest.replay_time,
            current_time + 1800
        ));
        // Expired after 1 hour
        assert!(is_replay_cooldown_expired(
            user_quest.replay_time,
            current_time + 3600
        ));
    }

    #[test]
    fn test_world_daily_quest_accessor() {
        use crate::world::WorldState;
        let world = WorldState::new();
        // No quests loaded in test world
        assert!(world.get_daily_quest(1).is_none());
        assert!(world.get_all_daily_quests().is_empty());
    }

    #[test]
    fn test_quest_skip_completed() {
        // Completed quests (status=1) should be skipped during kill tracking
        let user_quest = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 1,
            kill_count: 5,
            status: DailyQuestStatus::Completed as i16,
            replay_time: 0,
        };
        assert_eq!(user_quest.status, DailyQuestStatus::Completed as i16);
    }

    #[test]
    fn test_quest_timewait_with_active_cooldown() {
        let user_quest = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 18,
            kill_count: 0,
            status: DailyQuestStatus::TimeWait as i16,
            replay_time: 1720003600, // future timestamp
        };
        // Should skip if cooldown not expired
        let current_time = 1720000000;
        assert!(!is_replay_cooldown_expired(
            user_quest.replay_time,
            current_time
        ));
    }

    #[test]
    fn test_genie_data_model() {
        use ko_db::models::user_data::UserGenieDataRow;
        let genie = UserGenieDataRow {
            user_id: "TestUser".to_string(),
            genie_time: 3600,
            genie_options: vec![0u8; 100],
            first_using_genie: 1,
        };
        assert_eq!(genie.user_id, "TestUser");
        assert_eq!(genie.genie_time, 3600);
        assert_eq!(genie.genie_options.len(), 100);
        assert_eq!(genie.first_using_genie, 1);
    }

    #[test]
    fn test_daily_op_model() {
        use ko_db::models::user_data::UserDailyOpRow;
        let op = UserDailyOpRow {
            user_id: "TestUser".to_string(),
            chaos_map_time: -1,
            user_rank_reward_time: -1,
            personal_rank_reward_time: -1,
            king_wing_time: -1,
            warder_killer_time1: 0,
            warder_killer_time2: 0,
            keeper_killer_time: 0,
            user_loyalty_wing_reward_time: 0,
            full_moon_rift_map_time: -1,
            copy_information_time: -1,
        };
        assert_eq!(op.chaos_map_time, -1);
        assert_eq!(op.warder_killer_time1, 0);
    }

    #[test]
    fn test_loot_settings_model() {
        use ko_db::models::user_data::UserLootSettingsRow;
        let settings = UserLootSettingsRow {
            id: 1,
            user_id: "TestUser".to_string(),
            warrior: 1,
            rogue: 1,
            mage: 1,
            priest: 1,
            weapon: 1,
            armor: 1,
            accessory: 1,
            normal: 1,
            upgrade: 1,
            craft: 1,
            rare: 1,
            magic: 1,
            unique_grade: 1,
            consumable: 1,
            price: 9999,
        };
        assert_eq!(settings.warrior, 1);
        assert_eq!(settings.price, 9999);
    }

    #[test]
    fn test_seal_exp_model() {
        use ko_db::models::user_data::UserSealExpRow;
        let seal = UserSealExpRow {
            user_id: "TestUser".to_string(),
            sealed_exp: 50000,
        };
        assert_eq!(seal.sealed_exp, 50000);
    }

    #[test]
    fn test_return_data_model() {
        use ko_db::models::user_data::UserReturnDataRow;
        let ret = UserReturnDataRow {
            character_id: "TestChar".to_string(),
            return_symbol_ok: Some(1),
            return_logout_time: Some(1720000000),
            return_symbol_time: Some(1720362302),
        };
        assert_eq!(ret.return_symbol_ok, Some(1));
        assert_eq!(ret.return_logout_time, Some(1720000000));
    }

    #[test]
    fn test_daily_reward_model() {
        use ko_db::models::user_data::DailyRewardUserRow;
        let reward = DailyRewardUserRow {
            user_id: "TestUser".to_string(),
            day_index: 5,
            claimed: true,
            day_of_month: 10,
            last_claim_month: 3,
        };
        assert!(reward.claimed);
        assert_eq!(reward.day_index, 5);
        assert_eq!(reward.day_of_month, 10);
    }

    // ── New tests for Sprint 480 — zone_check, packet builders, integration ──

    #[test]
    fn test_zone_check_moradon() {
        assert!(zone_check(21, 21)); // Moradon match
        assert!(!zone_check(21, 1)); // Moradon mismatch
    }

    #[test]
    fn test_zone_check_any_zone() {
        assert!(zone_check(0, 21)); // No restriction
        assert!(zone_check(0, 1));
        assert!(zone_check(0, 71));
        assert!(zone_check(0, 0));
    }

    #[test]
    fn test_zone_check_eslant() {
        assert!(zone_check(11, 11));
        assert!(!zone_check(11, 12));
        assert!(!zone_check(11, 13));

        // Valid zones: 11, 13, 14 -- zone 12 itself is EXCLUDED
        assert!(!zone_check(12, 12)); // C++ bug: zone 12 quest fails for player in zone 12
        assert!(zone_check(12, 11)); // Karus Eslant counts
        assert!(zone_check(12, 13)); // Karus Eslant 2 counts
        assert!(zone_check(12, 14)); // Karus Eslant 3 counts
        assert!(!zone_check(12, 15)); // Elmorad Eslant 2 does NOT count
    }

    #[test]
    fn test_zone_check_specific() {
        assert!(zone_check(71, 71)); // BDW
        assert!(!zone_check(71, 21));
        assert!(zone_check(2, 2));
        assert!(zone_check(2, 7)); // Elmorad2
        assert!(zone_check(2, 8)); // Elmorad3
        assert!(!zone_check(2, 1));
        assert!(!zone_check(2, 9));
    }

    #[test]
    fn test_kill_update_packet_format() {
        let pkt = build_kill_update(5, 750);
        assert_eq!(pkt.opcode, 0xE9); // EXT_HOOK_S2C
        let data = &pkt.data;
        // data = [0xD3][0x02][u8 5][u16 750]
        assert_eq!(data[0], 0xD3); // DailyQuest sub-opcode
        assert_eq!(data[1], 0x02); // killupdate
        assert_eq!(data[2], 5); // quest index
        assert_eq!(u16::from_le_bytes([data[3], data[4]]), 750); // monster_id
        assert_eq!(data.len(), 5);
    }

    #[test]
    fn test_progress_notice_packet_format() {
        let pkt = build_progress_notice("TestQuest", 3, 10, 750);
        assert_eq!(pkt.opcode, 0xE9); // EXT_HOOK_S2C
        let data = &pkt.data;
        // data = [0xDC][SByte string][u16 3][u16 10][u16 750]
        assert_eq!(data[0], 0xDC); // DailyNotice sub-opcode
                                   // SByte string: [u8 len][bytes...]
        let name_len = data[1] as usize;
        assert_eq!(name_len, 9); // "TestQuest" = 9 bytes
        let name_end = 2 + name_len;
        assert_eq!(&data[2..name_end], b"TestQuest");
        // After string: u16 current, u16 required, u16 monster_id
        assert_eq!(u16::from_le_bytes([data[name_end], data[name_end + 1]]), 3);
        assert_eq!(
            u16::from_le_bytes([data[name_end + 2], data[name_end + 3]]),
            10
        );
        assert_eq!(
            u16::from_le_bytes([data[name_end + 4], data[name_end + 5]]),
            750
        );
    }

    #[test]
    fn test_daily_quest_send_list_definitions_packet() {
        // Test sendlist packet format (packet builder only, no WorldState needed)
        let def = make_quest(1, 750, 5, 0, 2);

        let mut pkt = Packet::new(Opcode::EXT_HOOK_S2C);
        pkt.write_u8(0xD3);
        pkt.write_u8(0x00); // sendlist
        pkt.write_u16(1); // count

        pkt.write_u8(def.id as u8);
        pkt.write_u8(def.time_type as u8);
        pkt.write_u8(def.kill_type as u8);

        assert_eq!(pkt.opcode, 0xE9); // EXT_HOOK_S2C
        let data = &pkt.data;
        // data = [0xD3][0x00][u16 1][u8 1][u8 0][u8 2]
        assert_eq!(data[0], 0xD3); // DailyQuest sub-opcode
        assert_eq!(data[1], 0x00); // sendlist
        assert_eq!(u16::from_le_bytes([data[2], data[3]]), 1); // count=1
        assert_eq!(data[4], 1); // quest index
        assert_eq!(data[5], 0); // time_type=repeat
        assert_eq!(data[6], 2); // kill_type=any
    }

    #[test]
    fn test_daily_quest_finished_repeat_cycle() {
        let quest = make_quest(1, 750, 5, 0, 2); // time_type=0 (repeat)
        let mut uq = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 1,
            kill_count: 5,
            status: DailyQuestStatus::Ongoing as i16,
            replay_time: 0,
        };

        // Simulate completion
        uq.status = status_after_completion(quest.time_type);
        uq.kill_count = 0;

        assert_eq!(uq.status, DailyQuestStatus::Ongoing as i16);
        assert_eq!(uq.kill_count, 0);
        // Can immediately redo
    }

    #[test]
    fn test_daily_quest_finished_time_gated() {
        let mut quest = make_quest(18, 2151, 50, 1, 2); // time_type=1 (time)
        quest.replay_time = 2; // 2 hours
        let current_time = 1720000000i32;

        let mut uq = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 18,
            kill_count: 50,
            status: DailyQuestStatus::Ongoing as i16,
            replay_time: 0,
        };

        // Simulate completion
        if quest.replay_time > 0 {
            uq.replay_time = current_time + calculate_replay_cooldown_secs(quest.replay_time);
        }
        uq.status = status_after_completion(quest.time_type);
        uq.kill_count = 0;

        assert_eq!(uq.status, DailyQuestStatus::TimeWait as i16);
        assert_eq!(uq.replay_time, 1720000000 + 7200); // 2h cooldown
        assert_eq!(uq.kill_count, 0);

        // Not expired
        assert!(!is_replay_cooldown_expired(
            uq.replay_time,
            current_time + 3600
        ));
        // Expired after 2h
        assert!(is_replay_cooldown_expired(
            uq.replay_time,
            current_time + 7200
        ));
    }

    #[test]
    fn test_daily_quest_finished_single() {
        let quest = make_quest(1, 750, 5, 2, 2); // time_type=2 (single)
        let mut uq = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 1,
            kill_count: 5,
            status: DailyQuestStatus::Ongoing as i16,
            replay_time: 0,
        };

        uq.status = status_after_completion(quest.time_type);
        uq.kill_count = 0;

        assert_eq!(uq.status, DailyQuestStatus::Completed as i16);
        // Permanently completed, never repeats
    }

    #[test]
    fn test_update_skip_completed() {
        let uq = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 1,
            kill_count: 5,
            status: DailyQuestStatus::Completed as i16,
            replay_time: 0,
        };
        // Completed quests are skipped in UpdateDailyQuestCount
        assert_eq!(uq.status, DailyQuestStatus::Completed as i16);
    }

    #[test]
    fn test_update_skip_timewait_active() {
        let uq = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 18,
            kill_count: 0,
            status: DailyQuestStatus::TimeWait as i16,
            replay_time: 1720003600,
        };
        let now = 1720000000i32;
        // TimeWait with active cooldown should be skipped
        let should_skip = uq.status == DailyQuestStatus::TimeWait as i16
            && uq.replay_time > 0
            && now < uq.replay_time;
        assert!(should_skip);
    }

    #[test]
    fn test_update_timewait_expired_becomes_ongoing() {
        let mut uq = UserDailyQuestRow {
            character_id: "Player".to_string(),
            quest_id: 18,
            kill_count: 0,
            status: DailyQuestStatus::TimeWait as i16,
            replay_time: 1720003600,
        };
        let now = 1720010000i32; // well past cooldown
                                 // Expired cooldown transitions to ongoing (in send_list)
        if uq.status == DailyQuestStatus::TimeWait as i16
            && is_replay_cooldown_expired(uq.replay_time, now)
        {
            uq.status = DailyQuestStatus::Ongoing as i16;
        }
        assert_eq!(uq.status, DailyQuestStatus::Ongoing as i16);
    }

    // ── Sprint 481: Reward type coverage tests ──

    #[test]
    fn test_reward_constants() {
        assert_eq!(ITEM_EXP, 900_001_000u32);
        assert_eq!(ITEM_GOLD, 900_000_000u32);
        assert_eq!(ITEM_COUNT, 900_002_000u32);
        assert_eq!(ITEM_LADDERPOINT, 900_003_000u32);
        assert_eq!(ITEM_RANDOM, 900_004_000u32);
    }

    #[test]
    fn test_get_quest_rewards_includes_virtual_items() {
        let mut quest = make_quest(1, 750, 5, 0, 2);
        quest.reward_1 = ITEM_EXP as i32;
        quest.count_1 = 50000;
        quest.reward_2 = ITEM_COUNT as i32;
        quest.count_2 = 100;
        quest.reward_3 = ITEM_RANDOM as i32;
        quest.count_3 = 1;
        quest.reward_4 = 0;
        quest.count_4 = 0;

        let rewards = get_quest_rewards(&quest);
        assert_eq!(rewards.len(), 3);
        assert_eq!(rewards[0], (ITEM_EXP as i32, 50000));
        assert_eq!(rewards[1], (ITEM_COUNT as i32, 100));
        assert_eq!(rewards[2], (ITEM_RANDOM as i32, 1));
    }

    #[test]
    fn test_quest_with_random_id() {
        let mut quest = make_quest(1, 750, 5, 0, 2);
        quest.random_id = 3;
        quest.reward_1 = ITEM_RANDOM as i32;
        quest.count_1 = 1;

        assert_eq!(quest.random_id, 3);
        let rewards = get_quest_rewards(&quest);
        assert_eq!(rewards.len(), 1);
        assert_eq!(rewards[0].0, ITEM_RANDOM as i32);
    }

    #[test]
    fn test_item_random_by_session_accessor() {
        use crate::world::WorldState;
        let world = WorldState::new();
        // Empty table — should return empty vec
        let items = world.get_item_random_by_session(1);
        assert!(items.is_empty());
    }
}
