//! NPC loot drop generation -- creates ground bundles when monsters die.
//! When a monster dies, the server:
//! 1. Checks `is_show_box()` to see if this NPC type drops loot
//! 2. Looks up the drop table (monster_item or npc_item) by template.item_table
//! 3. Generates gold (70-100% of template money)
//! 4. Rolls each of 12 item slots against their drop percentage
//! 5. Creates a GroundBundle at the NPC's position
//! 6. Sends WIZ_ITEM_DROP to the killer (or party)

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use ko_protocol::{Opcode, Packet};
use rand::Rng;

use crate::npc::{NpcId, NpcInstance, NpcTemplate};
use crate::npc_type_constants::{
    NPC_BATTLE_MONUMENT, NPC_BIFROST_MONUMENT, NPC_CHAOS_STONE, NPC_CLAN_WAR_MONUMENT,
    NPC_ELMORAD_GATEKEEPER, NPC_ELMORAD_WARDER1, NPC_ELMORAD_WARDER2, NPC_GUARD_TOWER1,
    NPC_GUARD_TOWER2, NPC_HUMAN_MONUMENT, NPC_KARUS_GATEKEEPER, NPC_KARUS_MONUMENT,
    NPC_KARUS_WARDER1, NPC_KARUS_WARDER2, NPC_PVP_MONUMENT, NPC_SCARECROW,
};
use crate::world::types::ZONE_RONARK_LAND_BASE;
use crate::world::{
    GroundBundle, LootItem, PremiumProperty, WorldState, ITEM_GOLD, NPC_HAVE_ITEM_LIST,
};
use crate::zone::SessionId;

/// Max items in a loot drop table (C++ LOOT_DROP_ITEMS = 12).
const LOOT_DROP_ITEMS: usize = 12;

/// Arrow stack count for arrow drops.
const _ARROW_STACK: u16 = 20;

/// Guard summon NPC proto ID -- siege-related guard, no loot.
const GUARD_SUMMON: u16 = 8850;

/// Monster proto IDs that never drop loot in Juraid Mountain.
const JURAID_NO_LOOT_PROTOS: &[u16] = &[
    2152, // MONSTER_APOSTLE_SSID
    8007, // MONSTER_DOOM_SOLDIER_SSID
    1772, // MONSTER_TROLL_CAPTAIN_SSID
];

/// Check if an NPC type should drop loot.
/// Returns false for non-lootable NPC types, event zones, guard summons,
/// and specific Juraid Mountain monsters.
pub fn is_show_box(npc_type: u8, zone_id: u16, proto_id: u16) -> bool {
    // NPC types that never drop loot (from C++ globals.h)
    const NO_LOOT_TYPES: &[u8] = &[
        NPC_GUARD_TOWER1,
        NPC_GUARD_TOWER2,
        NPC_KARUS_MONUMENT,
        NPC_HUMAN_MONUMENT,
        NPC_BIFROST_MONUMENT,
        NPC_SCARECROW,
        NPC_KARUS_WARDER1,
        NPC_KARUS_WARDER2,
        NPC_ELMORAD_WARDER1,
        NPC_ELMORAD_WARDER2,
        NPC_KARUS_GATEKEEPER,
        NPC_ELMORAD_GATEKEEPER,
        NPC_CHAOS_STONE,
        NPC_PVP_MONUMENT,
        NPC_BATTLE_MONUMENT,
        NPC_CLAN_WAR_MONUMENT,
    ];

    // Zones where loot is disabled
    const NO_LOOT_ZONES: &[u16] = &[
        55, // ZONE_FORGOTTEN_TEMPLE
        89, // ZONE_DUNGEON_DEFENCE
        92, // ZONE_PRISON
    ];

    if NO_LOOT_TYPES.contains(&npc_type) {
        return false;
    }
    if NO_LOOT_ZONES.contains(&zone_id) {
        return false;
    }
    // Guard summon NPCs never drop loot
    if proto_id == GUARD_SUMMON {
        return false;
    }
    // Specific monsters in Juraid Mountain never drop loot
    if zone_id == ZONE_RONARK_LAND_BASE && JURAID_NO_LOOT_PROTOS.contains(&proto_id) {
        return false;
    }
    true
}

/// Extract item/percent slot pairs from a `MonsterItemRow`.
/// Both `MonsterItemRow` and `NpcItemRow` have identical item01-12 + percent01-12 fields.
/// This helper extracts them into an indexable array for iteration.
fn extract_drop_slots(table: &ko_db::models::MonsterItemRow) -> [(i32, i16); LOOT_DROP_ITEMS] {
    [
        (table.item01, table.percent01),
        (table.item02, table.percent02),
        (table.item03, table.percent03),
        (table.item04, table.percent04),
        (table.item05, table.percent05),
        (table.item06, table.percent06),
        (table.item07, table.percent07),
        (table.item08, table.percent08),
        (table.item09, table.percent09),
        (table.item10, table.percent10),
        (table.item11, table.percent11),
        (table.item12, table.percent12),
    ]
}

/// Convert an `NpcItemRow` into a `MonsterItemRow` for uniform processing.
/// Both structs have identical field layouts (s_index + 12 item/percent pairs).
fn npc_item_to_monster_item(npc_row: &ko_db::models::NpcItemRow) -> ko_db::models::MonsterItemRow {
    ko_db::models::MonsterItemRow {
        s_index: npc_row.s_index,
        item01: npc_row.item01,
        percent01: npc_row.percent01,
        item02: npc_row.item02,
        percent02: npc_row.percent02,
        item03: npc_row.item03,
        percent03: npc_row.percent03,
        item04: npc_row.item04,
        percent04: npc_row.percent04,
        item05: npc_row.item05,
        percent05: npc_row.percent05,
        item06: npc_row.item06,
        percent06: npc_row.percent06,
        item07: npc_row.item07,
        percent07: npc_row.percent07,
        item08: npc_row.item08,
        percent08: npc_row.percent08,
        item09: npc_row.item09,
        percent09: npc_row.percent09,
        item10: npc_row.item10,
        percent10: npc_row.percent10,
        item11: npc_row.item11,
        percent11: npc_row.percent11,
        item12: npc_row.item12,
        percent12: npc_row.percent12,
    }
}

/// Generate loot for a killed NPC and create a ground bundle.
/// Returns the bundle_id if loot was generated, None if no loot.
pub fn generate_npc_loot(
    world: &WorldState,
    killer_sid: SessionId,
    npc_id: NpcId,
    npc: &NpcInstance,
    tmpl: &NpcTemplate,
) -> Option<u32> {
    let mut rng = rand::thread_rng();

    // Get drop table for this NPC
    let drop_table = if tmpl.item_table == 0 {
        None
    } else if tmpl.is_monster {
        world.get_monster_item(tmpl.item_table)
    } else {
        // NPC items use the same structure -- convert NpcItemRow to MonsterItemRow
        world
            .get_npc_item(tmpl.item_table)
            .map(|npc_row| npc_item_to_monster_item(&npc_row))
    };

    let mut items: [LootItem; NPC_HAVE_ITEM_LIST] = Default::default();
    let mut item_count: u8 = 0;

    // -- Slot 0: Gold --
    if tmpl.money > 0 {
        // C++ generates 70-100% of m_iMoney, then caps at SHRT_MAX (32000).
        let gold_pct = rng.gen_range(70..=100);
        let mut gold_amount = ((tmpl.money as u64 * gold_pct) / 100).min(32000) as u32;

        // NOTE: noah_gain + item_gold bonus is applied at PICKUP time via GoldGain(bApplyBonus=true),
        // NOT at loot generation time. C++ Npc.cpp:7700-7705 only does money * random / 100.

        // Apply coin event amount (GM-set server-wide gold multiplier) AFTER the 32000 cap.
        // C++ caps the final result at USHRT_MAX (65535), not 32000.
        let coin_event = world
            .game_time_weather()
            .coin_event_amount
            .load(Ordering::Relaxed);
        if coin_event > 0 {
            gold_amount = gold_amount * (100 + coin_event as u32) / 100;
        }

        let gold_amount = gold_amount.min(u16::MAX as u32) as u16;
        if gold_amount > 0 {
            items[0] = LootItem {
                item_id: ITEM_GOLD,
                count: gold_amount,
                slot_id: 0,
            };
            item_count += 1;
        }
    }

    // -- Slots 1-7: Item drops --
    if let Some(ref table) = drop_table {
        let drop_slots = extract_drop_slots(table);

        // Get killer's nation for item_production
        let killer_nation = world
            .get_character_info(killer_sid)
            .map(|ch| ch.nation)
            .unwrap_or(0);

        for &(item_id, percent) in &drop_slots {
            if item_count as usize >= NPC_HAVE_ITEM_LIST {
                break;
            }
            if item_id == 0 || percent == 0 {
                continue;
            }

            // Apply premium and event drop rate bonuses.
            //   1) premium drop (additive)
            //   2) drop scroll (multiplicative)
            //   3) clan premium (additive)
            //   4) flame level bonus (additive)
            //   5) drop event (multiplicative)
            let mut adjusted_percent = percent as i32;

            // 1) Premium drop (additive): iPer += iPer * pers1 / 100
            let prem_drop = world.get_premium_property(killer_sid, PremiumProperty::DropPercent);
            if prem_drop > 0 {
                adjusted_percent += adjusted_percent * prem_drop / 100;
            }

            // 2) Drop scroll (multiplicative): iPer = iPer * (100 + scroll) / 100
            let scroll_amount = world
                .with_session(killer_sid, |h| h.drop_scroll_amount)
                .unwrap_or(0);
            if scroll_amount > 0 {
                adjusted_percent = adjusted_percent * (100 + scroll_amount as i32) / 100;
            }

            // 3) Clan premium (additive): iPer += iPer * pers2 / 100
            let clan_drop =
                world.get_clan_premium_property(killer_sid, PremiumProperty::DropPercent);
            if clan_drop > 0 {
                adjusted_percent += adjusted_percent * clan_drop / 100;
            }

            // 4) Flame level bonus (additive): iPer += iPer * droprate / 100
            let flame_level = crate::systems::flash::get_flame_level(world, killer_sid);
            if flame_level > 0 {
                if let Some(feat) = world.get_burning_feature(flame_level) {
                    if feat.drop_rate > 0 {
                        adjusted_percent += adjusted_percent * feat.drop_rate as i32 / 100;
                    }
                }
            }

            // 5) Drop event (multiplicative): iPer = iPer * (100 + event) / 100
            let drop_event = world
                .game_time_weather()
                .drop_event_amount
                .load(Ordering::Relaxed);
            if drop_event > 0 {
                adjusted_percent = adjusted_percent * (100 + drop_event as i32) / 100;
            }

            // 6) Perk percentDrop bonus (additive): iPer += iPer * perkDrop / 100
            let perk_drop = world
                .with_session(killer_sid, |h| {
                    world.compute_perk_bonus(&h.perk_levels, 4, false)
                })
                .unwrap_or(0);
            if perk_drop > 0 {
                adjusted_percent += adjusted_percent * perk_drop / 100;
            }

            // C++ caps iPer at 10000
            if adjusted_percent > 10000 {
                adjusted_percent = 10000;
            }

            // Roll drop chance (C++ myrand(0, 10000) < iPer)
            let roll = rng.gen_range(0..10000);
            if roll >= adjusted_percent {
                continue;
            }

            let (resolved_id, count) = if item_id >= 100_000_000 {
                // Direct item ID
                (item_id as u32, 1u16)
            } else if item_id < 100 {
                // Grade code -- resolve via item_production
                let produced = super::item_production::item_production(
                    world,
                    item_id,
                    tmpl.level as i32,
                    killer_nation,
                );
                if produced == 0 {
                    continue;
                }
                (produced, 1u16)
            } else {
                // MakeItemGroup (100 <= id < 100_000_000)
                if let Some(group) = world.get_make_item_group(item_id) {
                    if group.items.is_empty() {
                        continue;
                    }
                    // C++ myrand(1, size) - 1 → uniform [0, size-1]
                    let idx = rng.gen_range(0..group.items.len());
                    let resolved = group.items[idx];
                    if resolved == 0 {
                        continue;
                    }
                    (resolved as u32, 1u16)
                } else {
                    tracing::trace!(
                        "MakeItemGroup: group {} not found for NPC {}",
                        item_id,
                        npc.proto_id
                    );
                    continue;
                }
            };

            items[item_count as usize] = LootItem {
                item_id: resolved_id,
                count,
                slot_id: item_count as u16,
            };
            item_count += 1;
        }
    }

    // Nothing to drop?
    if item_count == 0 {
        return None;
    }

    // -- Create ground bundle --
    let bundle_id = world.allocate_bundle_id();
    let bundle = GroundBundle {
        bundle_id,
        items_count: item_count,
        npc_id: npc.proto_id,
        looter: killer_sid,
        x: npc.x,
        z: npc.z,
        y: npc.y,
        zone_id: npc.zone_id,
        drop_time: Instant::now(),
        items,
    };
    world.add_ground_bundle(bundle);

    // -- Send WIZ_ITEM_DROP to killer (and party) --
    // C++ format for NPC drops: [u32 npc_id][u32 bundle_id][u8 1]
    let mut drop_pkt = Packet::new(Opcode::WizItemDrop as u8);
    drop_pkt.write_u32(npc_id);
    drop_pkt.write_u32(bundle_id);
    drop_pkt.write_u8(1);

    // Send to killer's party or just the killer
    let party_id = world.get_party_id(killer_sid);
    let party = party_id.and_then(|pid| world.get_party(pid));

    if let Some(party) = party {
        for member_sid in party.active_members() {
            world.send_to_session(member_sid, &drop_pkt);
        }
    } else {
        world.send_to_session_owned(killer_sid, drop_pkt);
    }

    tracing::debug!(
        "NPC loot: npc_id={} bundle_id={} items={} (killer={})",
        npc_id,
        bundle_id,
        item_count,
        killer_sid
    );

    // ── Auto-loot: immediately pick up items if an eligible player has robin loot ──
    // for m_bAutoLoot, then call auto-loot bundle pickup (BundleSystem.cpp)
    try_auto_loot(world, killer_sid, bundle_id, npc);

    Some(bundle_id)
}

/// Get the next item routing user for party round-robin distribution.
/// Uses the party's `item_routing` cursor to find the next eligible member
/// who is alive, in range, and has weight/slot capacity.
/// Cursor increments BEFORE filtering (load balancing over time).
fn get_item_routing_user(
    world: &WorldState,
    party_id: u16,
    sender_sid: SessionId,
    item_id: u32,
    count: u16,
) -> Option<SessionId> {
    use super::INVENTORY_TOTAL;
    use crate::world::RANGE_50M;

    let party = world.get_party(party_id)?;
    let sender_pos = world.get_position(sender_sid)?;

    let mut found = None;
    let mut routing = party.item_routing;

    for _ in 0..8 {
        // C++ increments BEFORE checking — wraparound at 6→0
        if routing > 6 {
            routing = 0;
        } else {
            routing += 1;
        }

        let member_sid = match party.members[routing as usize] {
            Some(sid) => sid,
            None => continue,
        };

        // Single DashMap read: check alive + in-range (2 reads → 1)
        let alive_in_range = world
            .with_session(member_sid, |h| {
                let ch = h.character.as_ref()?;
                if ch.res_hp_type == crate::world::USER_DEAD || ch.hp <= 0 {
                    return None;
                }
                let dx = h.position.x - sender_pos.x;
                let dz = h.position.z - sender_pos.z;
                Some(dx * dx + dz * dz <= RANGE_50M)
            })
            .flatten()
            .unwrap_or(false);
        if !alive_in_range {
            continue;
        }

        // Weight and slot check
        if !world.check_weight(member_sid, item_id, count) {
            continue;
        }
        if world
            .find_slot_for_item(member_sid, item_id, count)
            .map(|p| p < INVENTORY_TOTAL)
            != Some(true)
        {
            continue;
        }

        found = Some(member_sid);
        break;
    }

    // Update party routing cursor
    let final_routing = routing;
    world.update_party(party_id, |p| {
        p.item_routing = final_routing;
    });

    found
}

/// Try to auto-loot a ground bundle for the killer or their party.
/// Checks killer and party members for `auto_loot` flag, then picks up all
/// items in the bundle automatically. `fairy_check` blocks auto-loot.
const PET_AUTO_LOOT_ITEM_ID: u32 = 850_680_000;
const PET_LOOT_MODE: u8 = 8;

fn try_auto_loot(world: &WorldState, killer_sid: SessionId, bundle_id: u32, npc: &NpcInstance) {
    use super::{INVENTORY_TOTAL, SLOT_MAX};
    use crate::world::{COIN_MAX, ITEMCOUNT_MAX, ITEM_GOLD, RANGE_50M};

    // Find the auto-loot user: check killer first, then party members.
    let party_id = world.get_party_id(killer_sid);
    let party = party_id.and_then(|pid| world.get_party(pid));

    // C++ Npc.cpp:7934-7982 — party scan checks ONLY m_bAutoLoot (NOT fairy_check).
    // fairy_check is checked later inside auto-loot bundle pickup (BundleSystem.cpp:43).
    let is_pet_loot_eligible = |member_sid: SessionId| -> bool {
        world
            .with_session(member_sid, |h| {
                let pet_ok = h
                    .pet_data
                    .as_ref()
                    .map(|pet| {
                        pet.nid != 0
                            && pet.state_change == PET_LOOT_MODE
                            && pet
                                .items
                                .iter()
                                .any(|slot| slot.item_id == PET_AUTO_LOOT_ITEM_ID)
                    })
                    .unwrap_or(false);

                let dx = h.position.x - npc.x;
                let dz = h.position.z - npc.z;

                pet_ok && dx * dx + dz * dz <= RANGE_50M * 4.0
            })
            .unwrap_or(false)
    };

    let auto_loot_user = if let Some(ref party) = party {
        let mut found = None;

        for &member_sid in &party.active_members() {
            let normal_auto_loot = world
                .with_session(member_sid, |h| {
                    if !h.auto_loot {
                        return false;
                    }

                    let dx = h.position.x - npc.x;
                    let dz = h.position.z - npc.z;
                    dx * dx + dz * dz <= RANGE_50M * 4.0
                })
                .unwrap_or(false);

            if normal_auto_loot || is_pet_loot_eligible(member_sid) {
                found = Some(member_sid);
                break;
            }
        }

        found
    } else {
        let normal_auto_loot = world
            .with_session(killer_sid, |h| h.auto_loot)
            .unwrap_or(false);

        if normal_auto_loot || is_pet_loot_eligible(killer_sid) {
            Some(killer_sid)
        } else {
            None
        }
    };

    tracing::info!(
        "PET_LOOT_SCAN killer={} bundle={} eligible={:?}",
        killer_sid,
        bundle_id,
        auto_loot_user
    );

    let looter_sid = match auto_loot_user {
        Some(sid) => sid,
        None => return, // No eligible auto-loot user
    };

    // C++ BundleSystem.cpp:43 — fairy_check blocks auto-loot inside bundle pickup
    let (fairy_blocks, pet_loot_active) = world
        .with_session(looter_sid, |h| {
            let pet_loot_active = h
                .pet_data
                .as_ref()
                .map(|pet| {
                    pet.nid != 0
                        && pet.state_change == PET_LOOT_MODE
                        && pet
                            .items
                            .iter()
                            .any(|slot| slot.item_id == PET_AUTO_LOOT_ITEM_ID)
                })
                .unwrap_or(false);

            (h.fairy_check, pet_loot_active)
        })
        .unwrap_or((false, false));

    if fairy_blocks && !pet_loot_active {
        return;
    }

    // Normal player auto-loot respects the zone flag.
    // A summoned pet with item 850680000 and MODE_LOOTING is explicitly
    // allowed to loot regardless of the normal zone auto_loot setting.
    let zone_allows = world
        .get_zone(npc.zone_id)
        .and_then(|z| z.zone_info.as_ref().map(|zi| zi.abilities.auto_loot))
        .unwrap_or(false);

    if !zone_allows && !pet_loot_active {
        tracing::debug!(
            "Auto-loot rejected: zone={} bundle={} zone_allows=false pet_loot=false",
            npc.zone_id,
            bundle_id
        );
        return;
    }

    // Pick up each item in the bundle
    for slot_id in 0..NPC_HAVE_ITEM_LIST as u16 {
        let taken = world.try_take_bundle_item(bundle_id, slot_id);
        let (item_id, count) = match taken {
            Some((id, cnt)) if id != 0 => (id, cnt),
            _ => continue,
        };

        if item_id == ITEM_GOLD {
            // Gold: distribute to party or give to looter
            if let Some(ref party) = party {
                let mut eligible: Vec<SessionId> = Vec::with_capacity(8);
                for &member_sid in &party.active_members() {
                    // Single DashMap read: check alive + in-range (2 reads → 1)
                    let in_range = world
                        .with_session(member_sid, |h| {
                            let ch = h.character.as_ref()?;
                            if ch.res_hp_type == crate::world::USER_DEAD || ch.hp <= 0 {
                                return None;
                            }
                            let dx = h.position.x - npc.x;
                            let dz = h.position.z - npc.z;
                            Some(dx * dx + dz * dz <= RANGE_50M)
                        })
                        .flatten()
                        .unwrap_or(false);
                    if in_range {
                        eligible.push(member_sid);
                    }
                }
                if eligible.is_empty() {
                    continue;
                }
                let share = (count as f32 / eligible.len() as f32) as u32;
                for &member_sid in &eligible {
                    if !world.try_jackpot_noah(member_sid, share) {
                        world.gold_gain_with_bonus_silent(member_sid, share);
                    }
                    let gold = world
                        .get_character_info(member_sid)
                        .map(|ch| ch.gold)
                        .unwrap_or(0);
                    let mut pkt = Packet::new(Opcode::WizItemGet as u8);
                    pkt.write_u8(2); // LOOT_PARTY_COIN_DISTRIBUTION
                    pkt.write_u32(bundle_id);
                    pkt.write_u8(0xFF);
                    pkt.write_u32(ITEM_GOLD);
                    pkt.write_u32(gold);
                    pkt.write_u16(slot_id);
                    world.send_to_session_owned(member_sid, pkt);
                }
            } else {
                let current = world
                    .get_character_info(looter_sid)
                    .map(|ch| ch.gold)
                    .unwrap_or(0);
                if current as u64 + count as u64 > COIN_MAX as u64 {
                    world.restore_bundle_item(bundle_id, slot_id, item_id, count);
                    continue;
                }
                if !world.try_jackpot_noah(looter_sid, count as u32) {
                    // false = don't send WIZ_GOLD_CHANGE (LOOT_SOLO packet handles it)
                    world.gold_gain_with_bonus_silent(looter_sid, count as u32);
                }
                let gold = world
                    .get_character_info(looter_sid)
                    .map(|ch| ch.gold)
                    .unwrap_or(0);
                let mut pkt = Packet::new(Opcode::WizItemGet as u8);
                pkt.write_u8(1); // LOOT_SOLO
                pkt.write_u32(bundle_id);
                pkt.write_i8(-1);
                pkt.write_u32(item_id);
                pkt.write_u16(count);
                pkt.write_u32(gold);
                pkt.write_u16(slot_id);
                world.send_to_session_owned(looter_sid, pkt);
            }
            continue;
        }

        // Non-gold item: determine receiver via party routing or looter
        let receiver = match party_id {
            Some(pid) if party.is_some() => {
                get_item_routing_user(world, pid, looter_sid, item_id, count).unwrap_or(looter_sid)
            }
            _ => looter_sid,
        };

        if !world.check_weight(receiver, item_id, count) {
            world.restore_bundle_item(bundle_id, slot_id, item_id, count);
            continue;
        }

        let dst_pos = match world.find_slot_for_item(receiver, item_id, count) {
            Some(p) if p < INVENTORY_TOTAL => p,
            _ => {
                world.restore_bundle_item(bundle_id, slot_id, item_id, count);
                continue;
            }
        };

        let item_def = match world.get_item(item_id) {
            Some(i) => i,
            None => {
                world.restore_bundle_item(bundle_id, slot_id, item_id, count);
                continue;
            }
        };

        let serial = world.generate_item_serial();
        let mut new_total: u16 = 0;
        let ok = world.update_inventory(receiver, |inv| {
            if dst_pos >= inv.len() {
                return false;
            }
            let s = &mut inv[dst_pos];
            let is_new = s.item_id == 0;
            s.item_id = item_id;
            s.count = s.count.saturating_add(count).min(ITEMCOUNT_MAX);
            if is_new {
                s.durability = item_def.duration.unwrap_or(0);
                s.serial_num = serial;
            }
            new_total = s.count;
            true
        });

        if !ok {
            world.restore_bundle_item(bundle_id, slot_id, item_id, count);
            continue;
        }

        world.set_user_ability(receiver);
        let gold = world
            .get_character_info(receiver)
            .map(|ch| ch.gold)
            .unwrap_or(0);
        let inv_pos = (dst_pos - SLOT_MAX) as u8;

        let mut pkt = Packet::new(Opcode::WizItemGet as u8);
        pkt.write_u8(1); // LOOT_SOLO
        pkt.write_u32(bundle_id);
        pkt.write_u8(inv_pos);
        pkt.write_u32(item_id);
        pkt.write_u16(new_total);
        pkt.write_u32(gold);
        pkt.write_u16(slot_id);
        world.send_to_session_owned(receiver, pkt);

        if let Some(pid) = party_id {
            if party.is_some() {
                let receiver_name = world
                    .get_character_info(receiver)
                    .map(|ch| ch.name.clone())
                    .unwrap_or_default();
                let mut notify = Packet::new(Opcode::WizItemGet as u8);
                notify.write_u8(3); // LOOT_PARTY_NOTIFICATION
                notify.write_u32(bundle_id);
                notify.write_u32(item_id);
                notify.write_sbyte_string(&receiver_name);
                notify.write_u16(slot_id);
                world.send_to_party(pid, &notify);

                // LOOT_PARTY_ITEM_GIVEN_AWAY to the looter (if different from receiver)
                if receiver != looter_sid {
                    let mut away = Packet::new(Opcode::WizItemGet as u8);
                    away.write_u8(4); // LOOT_PARTY_ITEM_GIVEN_AWAY
                    world.send_to_session_owned(looter_sid, away);
                }
            }
        }

        // ── Drop Notice: server-wide broadcast for rare items (auto-loot) ──
        //   if (pTable.m_isDropNotice && g_pMain->pServerSetting.DropNotice && !isGM())
        // Note: C++ uses pReceiver->GetName() and this->GetLoyaltySymbolRank()
        if item_def.drop_notice.unwrap_or(0) != 0 && !world.is_gm(looter_sid) {
            let drop_notice_enabled = world
                .get_server_settings()
                .map(|s| s.drop_notice != 0)
                .unwrap_or(false);
            if drop_notice_enabled {
                let receiver_name = world.get_session_name(receiver).unwrap_or_default();
                let rank = world
                    .with_session(looter_sid, |h| h.personal_rank)
                    .unwrap_or(0);
                let notice = super::logosshout::build_drop_notice(&receiver_name, item_id, rank);
                world.broadcast_to_all(Arc::new(notice), None);
            }
        }
    }

    tracing::debug!(
        "Auto-loot: bundle_id={} looter={} pet_loot={}",
        bundle_id,
        looter_sid,
        pet_loot_active
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_show_box_normal_monster() {
        assert!(is_show_box(0, 1, 100)); // Normal monster type in normal zone
    }

    #[test]
    fn test_is_show_box_chaos_stone() {
        assert!(!is_show_box(NPC_CHAOS_STONE, 1, 100));
    }

    #[test]
    fn test_is_show_box_guard_tower() {
        assert!(!is_show_box(NPC_GUARD_TOWER1, 1, 100));
        assert!(!is_show_box(NPC_GUARD_TOWER2, 1, 100));
    }

    #[test]
    fn test_is_show_box_scarecrow() {
        assert!(!is_show_box(NPC_SCARECROW, 1, 100));
    }

    #[test]
    fn test_is_show_box_monuments() {
        assert!(!is_show_box(NPC_KARUS_MONUMENT, 1, 100));
        assert!(!is_show_box(NPC_HUMAN_MONUMENT, 1, 100));
        assert!(!is_show_box(NPC_BIFROST_MONUMENT, 1, 100));
        assert!(!is_show_box(NPC_PVP_MONUMENT, 1, 100));
        assert!(!is_show_box(NPC_BATTLE_MONUMENT, 1, 100));
        assert!(!is_show_box(NPC_CLAN_WAR_MONUMENT, 1, 100));
    }

    #[test]
    fn test_is_show_box_warders_gatekeepers() {
        assert!(!is_show_box(NPC_KARUS_WARDER1, 1, 100));
        assert!(!is_show_box(NPC_KARUS_WARDER2, 1, 100));
        assert!(!is_show_box(NPC_ELMORAD_WARDER1, 1, 100));
        assert!(!is_show_box(NPC_ELMORAD_WARDER2, 1, 100));
        assert!(!is_show_box(NPC_KARUS_GATEKEEPER, 1, 100));
        assert!(!is_show_box(NPC_ELMORAD_GATEKEEPER, 1, 100));
    }

    #[test]
    fn test_is_show_box_no_loot_zones() {
        assert!(!is_show_box(0, 55, 100)); // ZONE_FORGOTTEN_TEMPLE
        assert!(!is_show_box(0, 89, 100)); // ZONE_DUNGEON_DEFENCE
        assert!(!is_show_box(0, 92, 100)); // ZONE_PRISON
    }

    #[test]
    fn test_is_show_box_normal_zones() {
        assert!(is_show_box(0, 1, 100)); // Karus starting zone
        assert!(is_show_box(0, 21, 100)); // Moradon
        assert!(is_show_box(0, 71, 100)); // Ardream
    }

    #[test]
    fn test_is_show_box_guard_summon() {
        // Guard summon (proto 8850) never drops loot
        assert!(!is_show_box(0, 1, GUARD_SUMMON));
    }

    #[test]
    fn test_is_show_box_juraid_exclusions() {
        // Specific monsters in Juraid Mountain (zone 73) never drop loot
        assert!(!is_show_box(0, ZONE_RONARK_LAND_BASE, 2152)); // MONSTER_APOSTLE_SSID
        assert!(!is_show_box(0, ZONE_RONARK_LAND_BASE, 8007)); // MONSTER_DOOM_SOLDIER_SSID
        assert!(!is_show_box(0, ZONE_RONARK_LAND_BASE, 1772)); // MONSTER_TROLL_CAPTAIN_SSID
                                                               // Normal monster in Juraid IS allowed
        assert!(is_show_box(0, ZONE_RONARK_LAND_BASE, 100));
        // Same protos in other zones ARE allowed
        assert!(is_show_box(0, 1, 2152));
    }

    // ── Sprint 41: Gold Bonus & Drop Rate Tests ─────────────────────

    #[test]
    fn test_gold_bonus_formula_zero_bonus() {
        // 0% bonus should not change the value
        let base: u32 = 100;
        let bonus: u8 = 0;
        let result = if bonus > 0 {
            base * (100 + bonus as u32) / 100
        } else {
            base
        };
        assert_eq!(result, 100);
    }

    #[test]
    fn test_gold_bonus_formula_ten_percent() {
        let base: u32 = 100;
        let bonus: u8 = 10;
        let result = base * (100 + bonus as u32) / 100;
        assert_eq!(result, 110);
    }

    #[test]
    fn test_gold_bonus_formula_fifty_percent() {
        let base: u32 = 200;
        let bonus: u8 = 50;
        let result = base * (100 + bonus as u32) / 100;
        assert_eq!(result, 300);
    }

    #[test]
    fn test_gold_bonus_formula_max_u8() {
        // Extreme case: 255% bonus
        let base: u32 = 100;
        let bonus: u8 = 255;
        let result = base * (100 + bonus as u32) / 100;
        assert_eq!(result, 355);
    }

    #[test]
    fn test_gold_initial_cap_at_32000() {
        // Initial random gold caps at 32000 (SHRT_MAX)
        let money: u64 = 50000;
        let pct: u64 = 100;
        let result = (money * pct / 100).min(32000) as u32;
        assert_eq!(result, 32000);
    }

    #[test]
    fn test_gold_coin_event_can_exceed_32000() {
        // After initial 32000 cap, coin event can push gold higher
        let base: u32 = 32000;
        let coin_event: u8 = 50;
        let result = base * (100 + coin_event as u32) / 100;
        assert_eq!(result, 48000); // 32000 * 1.5 = 48000 (above 32000, below 65535)
    }

    #[test]
    fn test_gold_final_cap_at_u16_max() {
        // Final cap is USHRT_MAX (65535), not 32000
        let base: u32 = 32000;
        let coin_event: u8 = 255; // extreme event
        let result = (base * (100 + coin_event as u32) / 100).min(u16::MAX as u32);
        assert_eq!(result, 65535);
    }

    #[test]
    fn test_coin_event_amount_formula() {
        // C++ Npc.cpp:7911 — coinAmount = count * (100 + m_byCoinEventAmount) / 100
        let base: u32 = 100;
        let event: u8 = 25;
        let result = base * (100 + event as u32) / 100;
        assert_eq!(result, 125);
    }

    #[test]
    fn test_drop_rate_premium_boost() {
        // Premium drop rate 50% should boost base 100 to 150
        let base: i32 = 100;
        let prem: i32 = 50;
        let result = base + base * prem / 100;
        assert_eq!(result, 150);
    }

    #[test]
    fn test_drop_rate_clan_premium_stacks() {
        // Premium + clan premium should stack additively
        let base: i32 = 100;
        let prem: i32 = 30;
        let clan: i32 = 20;
        let mut adjusted = base;
        adjusted += adjusted * prem / 100; // 100 + 30 = 130
        adjusted += adjusted * clan / 100; // 130 + 26 = 156
        assert_eq!(adjusted, 156);
    }

    #[test]
    fn test_drop_rate_event_amount_stacks() {
        // Drop event amount is multiplicative: iPer * (100 + event) / 100
        let base: i32 = 200;
        let event: u8 = 50;
        let result = base * (100 + event as i32) / 100;
        assert_eq!(result, 300);
    }

    #[test]
    fn test_drop_rate_cap_at_10000() {
        // C++ caps iPer at 10000
        let base: i32 = 8000;
        let prem: i32 = 50;
        let mut adjusted = base;
        adjusted += adjusted * prem / 100; // 8000 + 4000 = 12000
        if adjusted > 10000 {
            adjusted = 10000;
        }
        assert_eq!(adjusted, 10000);
    }

    #[test]
    fn test_drop_rate_no_bonus_no_change() {
        let base: i32 = 500;
        let prem: i32 = 0;
        let clan: i32 = 0;
        let event: u8 = 0;
        let mut adjusted = base;
        if prem > 0 {
            adjusted += adjusted * prem / 100;
        }
        if clan > 0 {
            adjusted += adjusted * clan / 100;
        }
        if event > 0 {
            adjusted = adjusted * (100 + event as i32) / 100;
        }
        assert_eq!(adjusted, 500);
    }

    #[test]
    fn test_gold_bonus_and_coin_event_combined() {
        // Both bonuses stacking
        let base: u32 = 100;
        let item_bonus: u8 = 20;
        let coin_event: u8 = 30;
        let mut gold = base;
        gold = gold * (100 + item_bonus as u32) / 100; // 120
        gold = gold * (100 + coin_event as u32) / 100; // 156
        assert_eq!(gold, 156);
    }

    // ── Sprint 257: Flame level drop rate bonus test ─────────────────

    #[test]
    fn test_flame_level_drop_rate_bonus() {
        // Flame level 1 with drop_rate=20 should increase drop chance by 20%
        let base: i32 = 500;
        let drop_rate: u8 = 20;
        let adjusted = base + base * drop_rate as i32 / 100;
        assert_eq!(adjusted, 600); // 500 + 100 = 600
    }

    #[test]
    fn test_flame_level_zero_no_bonus() {
        // Flame level 0 should not change anything
        let base: i32 = 500;
        let flame_level: u16 = 0;
        let adjusted = if flame_level > 0 {
            base + base * 20 / 100 // some arbitrary drop rate
        } else {
            base
        };
        assert_eq!(adjusted, 500);
    }

    #[test]
    fn test_flame_level_drop_rate_pipeline_order() {
        // Full pipeline: premium(10%) -> scroll(0) -> clan(0) -> flame(20%) -> event(0)
        let base: i32 = 1000;
        let mut adjusted = base;

        // 1) premium 10%
        adjusted += adjusted * 10 / 100; // 1100

        // 2) drop scroll = 0 (skip)
        // 3) clan premium = 0 (skip)

        // 4) flame level drop_rate = 20%
        let drop_rate: u8 = 20;
        adjusted += adjusted * drop_rate as i32 / 100; // 1100 + 220 = 1320

        // 5) event = 0 (skip)

        assert_eq!(adjusted, 1320);
    }
}
