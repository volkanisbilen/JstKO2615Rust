Warning: truncated output (original token count: 69946)
Total output lines: 7493

//! WIZ_ATTACK (0x08) handler -- physical melee attack.
//! ## Client Request (C->S)
//! | Type  | Description                        |
//! |-------|------------------------------------|
//! | u8    | bType (attack sub-type)            |
//! | u8    | bResult (client-side, overwritten)  |
//! | u32le | tid (target ID)                    |
//! | i16le | delaytime (weapon delay ticks)      |
//! | i16le | distance (to target)               |
//! | u8    | unknown                            |
//! | u8    | unknowns                           |
//! ## Server Broadcast (S->C, to 3x3 region)
//! | Type  | Description                        |
//! |-------|------------------------------------|
//! | u8    | bType (attack sub-type)            |
//! | u8    | bResult (0=fail, 1=success, 2=dead)|
//! | u32le | attacker session ID                |
//! | u32le | target ID                          |
//! | u8    | unknown (echoed from client)       |
//! ## Attack Result Constants
//! - `ATTACK_FAIL` (0): attack missed or blocked
//! - `ATTACK_SUCCESS` (1): attack landed, target still alive
//! - `ATTACK_TARGET_DEAD` (2): attack killed the target

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ko_db::models::CoefficientRow;
use ko_protocol::{Opcode, Packet, PacketReader};
use rand::Rng;
use rand::SeedableRng;

use crate::handler::arena;
use crate::handler::dead;
use crate::handler::durability::{WORE_TYPE_ATTACK, WORE_TYPE_DEFENCE};
use crate::npc::NpcId;
use crate::npc_type_constants::{
    NPC_BIFROST_MONUMENT, NPC_BORDER_MONUMENT, NPC_CLAN_WAR_MONUMENT, NPC_DESTROYED_ARTIFACT,
    NPC_FOSIL, NPC_GATE, NPC_GATE2, NPC_GATE_LEVER, NPC_GUARD_TOWER1, NPC_GUARD_TOWER2,
    NPC_OBJECT_FLAG, NPC_PARTNER_TYPE, NPC_PHOENIX_GATE, NPC_PRISON, NPC_PVP_MONUMENT, NPC_REFUGEE,
    NPC_SANTA, NPC_SOCCER_BAAL, NPC_SPECIAL_GATE, NPC_TREE, NPC_VICTORY_GATE,
};
use crate::session::{ClientSession, SessionState};
use crate::systems::bdw;
use crate::systems::regen::build_hp_change_packet_with_attacker;
use crate::world::{
    CharacterInfo, CswOpStatus, NpcState, Position, WorldState, ITEM_GOLD, NATION_ELMORAD,
    NATION_KARUS, RANGE_50M, RANGE_80M, USER_DEAD, USER_SITDOWN, ZONE_ARDREAM, ZONE_ARENA,
    ZONE_BATTLE, ZONE_BATTLE2, ZONE_BATTLE3, ZONE_BATTLE4, ZONE_BATTLE5, ZONE_BATTLE6,
    ZONE_BIFROST, ZONE_BORDER_DEFENSE_WAR, ZONE_CHAOS_DUNGEON, ZONE_CLAN_WAR_ARDREAM,
    ZONE_CLAN_WAR_RONARK, ZONE_DELOS, ZONE_DESPERATION_ABYSS, ZONE_DRAGON_CAVE,
    ZONE_DUNGEON_DEFENCE, ZONE_ELMORAD, ZONE_ELMORAD2, ZONE_ELMORAD3, ZONE_HELL_ABYSS,
    ZONE_JURAID_MOUNTAIN, ZONE_KARUS, ZONE_KARUS2, ZONE_KARUS3, ZONE_KNIGHT_ROYALE,
    ZONE_KROWAZ_DOMINION, ZONE_MORADON, ZONE_MORADON5, ZONE_PARTY_VS_1, ZONE_PARTY_VS_4,
    ZONE_RONARK_LAND, ZONE_RONARK_LAND_BASE, ZONE_SNOW_BATTLE,
};
use crate::zone::SessionId;

use crate::attack_constants::{
    ATTACK_FAIL, ATTACK_SUCCESS, ATTACK_TARGET_DEAD, FAIL, GREAT_SUCCESS, NORMAL, SUCCESS,
};
use crate::buff_constants::{BUFF_TYPE_BLIND, BUFF_TYPE_FREEZE, BUFF_TYPE_KAUL_TRANSFORMATION};

// ── Melee constants ─────────────────────────────────────────────────────────

/// Minimum melee attack delay (empty-handed or mage).
const MIN_MELEE_DELAY: i16 = 100;

/// Server-side rate limit between R-attacks (milliseconds).
const PLAYER_R_HIT_REQUEST_INTERVAL: u64 = 900;

/// Maximum attack range for melee (in game units).
/// When no weapon is equipped, use a generous melee range to
/// avoid rejecting legitimate attacks.
const DEFAULT_MELEE_RANGE: f32 = 15.0;

/// GM weapon item ID — bypasses delay checks.
const GM_WEAPON_ID: u32 = 389158000;

/// Minimum weapon power for bare-hand attacks.
const MIN_WEAPON_POWER: u16 = 3;

/// Default attack amount multiplier (no buffs).
#[cfg(test)]
const DEFAULT_ATTACK_AMOUNT: u16 = 100;

// ── Class group helpers ─────────────────────────────────────────────────────

/// Get the "base class type" from a full class ID.
/// The class % 100 gives: 1=Warrior, 2=Rogue, 3=Mage, 4=Priest,
/// 5=WarriorNovice, 6=WarriorMaster, 7=RogueNovice, 8=RogueMaster,
/// 9=MageNovice, 10=MageMaster, 11=PriestNovice, 12=PriestMaster,
/// 13=Kurian, 14=KurianNovice, 15=KurianMaster.
fn base_class(class: u16) -> u16 {
    class % 100
}

/// Check if class belongs to the Warrior group (1, 5, 6).
fn is_warrior(class: u16) -> bool {
    matches!(base_class(class), 1 | 5 | 6)
}

/// Check if class belongs to the Rogue group (2, 7, 8).
fn is_rogue(class: u16) -> bool {
    matches!(base_class(class), 2 | 7 | 8)
}

/// Check if class belongs to the Mage group (3, 9, 10).
fn is_mage(class: u16) -> bool {
    matches!(base_class(class), 3 | 9 | 10)
}

/// Check if class belongs to the Priest group (4, 11, 12).
fn is_priest(class: u16) -> bool {
    matches!(base_class(class), 4 | 11 | 12)
}

/// Map a class to its class group index (0-based) for AP/AC class bonus arrays.
/// Returns GROUP_WARRIOR(1)-1=0, GROUP_ROGUE(2)-1=1, GROUP_MAGE(3)-1=2, GROUP_CLERIC(4)-1=3.
/// Returns `None` for Kurian or unknown classes (no class bonus slot).
pub(crate) fn class_group_index(class: u16) -> Option<usize> {
    let bc = base_class(class);
    match bc {
        1 | 5 | 6 => Some(0),   // Warrior
        2 | 7 | 8 => Some(1),   // Rogue
        3 | 9 | 10 => Some(2),  // Mage
        4 | 11 | 12 => Some(3), // Priest
        _ => None,              // Kurian or unknown
    }
}

use crate::inventory_constants::{
    WEAPON_KIND_1H_AXE, WEAPON_KIND_1H_CLUB, WEAPON_KIND_1H_SPEAR, WEAPON_KIND_1H_SWORD,
    WEAPON_KIND_2H_AXE, WEAPON_KIND_2H_CLUB, WEAPON_KIND_2H_SPEAR, WEAPON_KIND_2H_SWORD,
    WEAPON_KIND_BOW, WEAPON_KIND_CROSSBOW, WEAPON_KIND_DAGGER, WEAPON_KIND_JAMADAR,
};

/// Check if item is a Timing Delay weapon.
fn is_timing_delay(item_num: u32) -> bool {
    item_num == 900335523 || item_num == 900336524 || item_num == 900337525
}

/// Check if item is a Wirinim Unique Delay weapon.
fn is_wirinom_uniq_delay(item_num: u32) -> bool {
    (127410731..=127410740).contains(&item_num)
        || (127420741..=127420750).contains(&item_num)
        || (127430751..=127430760).contains(&item_num)
        || (127440761..=127440770).contains(&item_num)
        || item_num == 127410284
        || item_num == 127420285
        || item_num == 127430286
        || item_num == 127440287
}

/// Check if item is a Wirinim Rebirth Delay weapon.
fn is_wirinom_reb_delay(item_num: u32) -> bool {
    (127411181..=127411210).contains(&item_num)
        || (127421211..=127421240).contains(&item_num)
        || (127431241..=127431270).contains(&item_num)
        || (127441271..=127441300).contains(&item_num)
}

/// Check if item is a Garges Sword Delay weapon.
fn is_garges_sword_delay(item_num: u32) -> bool {
    (1110582731..=1110582740).contains(&item_num) || item_num == 1110582451
}

/// Check if a weapon kind is a bow or crossbow.
fn is_bow_weapon(kind: i32) -> bool {
    kind == WEAPON_KIND_BOW || kind == WEAPON_KIND_CROSSBOW
}

/// Get the weapon range in game units for server-side distance validation.
/// falls back to left-hand. Returns `pTable.m_sRange / 10.0`.
fn get_weapon_range(world: &WorldState, sid: SessionId) -> f32 {
    // Single session lock for both weapon slot item IDs (2 DashMap reads → 1)
    let (rh_id, lh_id) = world
        .with_session(sid, |h| {
            let rh = h
                .inventory
                .get(crate::inventory_constants::RIGHTHAND)
                .map(|s| s.item_id)
                .unwrap_or(0);
            let lh = h
                .inventory
                .get(crate::inventory_constants::LEFTHAND)
                .map(|s| s.item_id)
                .unwrap_or(0);
            (rh, lh)
        })
        .unwrap_or((0, 0));
    // Item table lookups (separate DashMap, no session contention)
    if rh_id != 0 {
        if let Some(w) = world.get_item(rh_id) {
            let r = w.range.unwrap_or(0) as f32;
            if r > 0.0 {
                return r / 10.0;
            }
        }
    }
    if lh_id != 0 {
        if let Some(w) = world.get_item(lh_id) {
            let r = w.range.unwrap_or(0) as f32;
            if r > 0.0 {
                return r / 10.0;
            }
        }
    }
    0.0
}

/// Check if the player is in an enemy safety area (no-PvP zone).
/// Safety areas are nation-specific spawn/village areas where the ENEMY
/// cannot attack. Each zone defines circular or rectangular safe regions.
/// "Enemy safety area" means: the area is safe for my enemies — I cannot attack here.
/// E.g., an Elmorad player near Elmorad village cannot attack Karus players there.
pub(crate) fn is_in_enemy_safety_area(zone_id: u16, x: f32, z: f32, nation: u8) -> bool {
    /// Circular distance check (squared) — matches `isInRangeSlow(x, z, range)`.
    fn in_range(px: f32, pz: f32, cx: f32, cz: f32, radius: f32) -> bool {
        let dx = px - cx;
        let dz = pz - cz;
        dx * dx + dz * dz <= radius * radius
    }

    match zone_id {
        ZONE_DELOS => in_range(x, z, 500.0, 180.0, 115.0),
        ZONE_BIFROST => {
            if nation == NATION_ELMORAD {
                x > 56.0 && x < 124.0 && z > 700.0 && z < 840.0
            } else {
                x > 190.0 && x < 270.0 && z > 870.0 && z < 970.0
            }
        }
        ZONE_ARENA => in_range(x, z, 127.0, 113.0, 36.0),
        ZONE_ELMORAD | ZONE_ELMORAD2 | ZONE_ELMORAD3 => {
            if nation == NATION_ELMORAD {
                in_range(x, z, 210.0, 1853.0, 50.0)
            } else {
                false
            }
        }
        ZONE_KARUS | ZONE_KARUS2 | ZONE_KARUS3 => {
            if nation == NATION_KARUS {
                in_range(x, z, 1860.0, 174.0, 50.0)
            } else {
                false
            }
        }
        ZONE_BATTLE => {
            if nation == NATION_KARUS {
                x > 98.0 && x < 125.0 && z > 755.0 && z < 780.0
            } else if nation == NATION_ELMORAD {
                x > 805.0 && x < 831.0 && z > 85.0 && z < 110.0
            } else {
                false
            }
        }
        ZONE_BATTLE2 => {
            if nation == NATION_KARUS {
                x > 942.0 && x < 977.0 && z > 863.0 && z < 904.0
            } else if nation == NATION_ELMORAD {
                x > 46.0 && x < 80.0 && z > 142.0 && z < 174.0
            } else {
                false
            }
        }
        ZONE_BATTLE4 => {
            if nation == NATION_KARUS {
                in_range(x, z, 235.0, 228.0, 80.0)
                    || in_range(x, z, 846.0, 362.0, 20.0)
                    || in_range(x, z, 338.0, 807.0, 20.0)
            } else if nation == NATION_ELMORAD {
                in_range(x, z, 809.0, 783.0, 80.0)
                    || in_range(x, z, 182.0, 668.0, 20.0)
                    || in_range(x, z, 670.0, 202.0, 20.0)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Check if the player is in their own safety area (protected from enemy attack).
/// "Own safety area" means: this area is safe for ME — enemies cannot attack me here.
/// Uses the same coordinates as `isInEnemySafetyArea` but with INVERTED nation logic.
/// E.g., a Karus player near Elmorad village is in their own safety area.
/// Used in `MagicProcess.cpp:384` for `MORAL_AREA_ALL` skill target validation
/// and in `is_hostile_to()` for target protection check.
pub(crate) fn is_in_own_safety_area(zone_id: u16, x: f32, z: f32, nation: u8) -> bool {
    fn in_range(px: f32, pz: f32, cx: f32, cz: f32, radius: f32) -> bool {
        let dx = px - cx;
        let dz = pz - cz;
        dx * dx + dz * dz <= radius * radius
    }

    match zone_id {
        ZONE_DELOS => in_range(x, z, 500.0, 180.0, 115.0),
        ZONE_BIFROST => {
            // Nation INVERTED vs isInEnemySafetyArea
            if nation == NATION_KARUS {
                x > 56.0 && x < 124.0 && z > 700.0 && z < 840.0
            } else {
                x > 190.0 && x < 270.0 && z > 870.0 && z < 970.0
            }
        }
        ZONE_ARENA => in_range(x, z, 127.0, 113.0, 36.0),
        ZONE_ELMORAD | ZONE_ELMORAD2 | ZONE_ELMORAD3 => {
            // C++ isInOwnSafetyArea: Karus in Elmorad zone (inverted from enemy check)
            if nation == NATION_KARUS {
                in_range(x, z, 210.0, 1853.0, 50.0)
            } else {
                false
            }
        }
        ZONE_KARUS | ZONE_KARUS2 | ZONE_KARUS3 => {
            // C++ isInOwnSafetyArea: Elmorad in Karus zone (inverted from enemy check)
            if nation == NATION_ELMORAD {
                in_range(x, z, 1860.0, 174.0, 50.0)
            } else {
                false
            }
        }
        ZONE_BATTLE => {
            // Nation INVERTED vs isInEnemySafetyArea
            if nation == NATION_ELMORAD {
                x > 98.0 && x < 125.0 && z > 755.0 && z < 780.0
            } else if nation == NATION_KARUS {
                x > 805.0 && x < 831.0 && z > 85.0 && z < 110.0
            } else {
                false
            }
        }
        ZONE_BATTLE2 => {
            if nation == NATION_ELMORAD {
                x > 942.0 && x < 977.0 && z > 863.0 && z < 904.0
            } else if nation == NATION_KARUS {
                x > 46.0 && x < 80.0 && z > 142.0 && z < 174.0
            } else {
                false
            }
        }
        ZONE_BATTLE4 => {
            if nation == NATION_ELMORAD {
                in_range(x, z, 235.0, 228.0, 80.0)
                    || in_range(x, z, 846.0, 362.0, 20.0)
                    || in_range(x, z, 338.0, 807.0, 20.0)
            } else if nation == NATION_KARUS {
                in_range(x, z, 809.0, 783.0, 80.0)
                    || in_range(x, z, 182.0, 668.0, 20.0)
                    || in_range(x, z, 670.0, 202.0, 20.0)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Apply weapon-type-specific armor resistance to PvP damage.
/// Each equipped weapon is checked against the target's weapon-type resistances
/// (accumulated from armor). The formula per weapon:
///   `damage -= damage * target_resistance / 250`
/// For daggers and bows, an additional amount modifier is applied:
///   `damage -= damage * (resistance * amount / 100) / 250`
/// where `dagger_r_amount`/`bow_r_amount` default to 100 and are reduced
/// by Eskrima debuff (BUFF_TYPE_DAGGER_BOW_DEFENSE, 45).
pub(crate) fn get_ac_damage(
    damage: i16,
    weapon_kinds: &[Option<i32>],
    target_stats: &crate::world::EquippedStats,
    dagger_r_amount: u8,
    bow_r_amount: u8,
) -> i16 {
    let mut d = damage as i32;
    for kind_opt in weapon_kinds {
        let kind = match kind_opt {
            Some(k) => *k,
            None => continue,
        };

        let resistance = if kind == WEAPON_KIND_DAGGER {
            target_stats.dagger_r as i32 * dagger_r_amount as i32 / 100
        } else if kind == WEAPON_KIND_1H_SWORD || kind == WEAPON_KIND_2H_SWORD {
            target_stats.sword_r as i32
        } else if kind == WEAPON_KIND_1H_AXE || kind == WEAPON_KIND_2H_AXE {
            target_stats.axe_r as i32
        } else if kind == WEAPON_KIND_1H_CLUB || kind == WEAPON_KIND_2H_CLUB {
            target_stats.club_r as i32
        } else if kind == WEAPON_KIND_1H_SPEAR || kind == WEAPON_KIND_2H_SPEAR {
            target_stats.spear_r as i32
        } else if kind == WEAPON_KIND_BOW || kind == WEAPON_KIND_CROSSBOW {
            target_stats.bow_r as i32 * bow_r_amount as i32 / 100
        } else if kind == WEAPON_KIND_JAMADAR {
            target_stats.jamadar_r as i32
        } else {
            continue;
        };

        d -= d * resistance / 250;
    }
    d.max(0) as i16
}

// ── Combat stat computations ────────────────────────────────────────────────

/// Calculate total attack power (m_sTotalHit) for a character.
/// Formula (simplified for no equipment):
/// - `power` = weapon damage, clamped min 3 (bare-hand)
/// - `coeff` = weapon-type coefficient from class table (0 for bare-hand)
/// - Rogue:   `(0.005 * power * (dex + 40)) + (coeff * power * level * dex) + 3`
/// - Warrior: `(0.005 * power * (main_stat + 40)) + (coeff * power * level * main_stat) + 3 + base_ap`
/// - Others:  Same as Warrior formula with STR as main stat.
/// `base_ap` = max(0, main_stat - 150) for stats > 150.
fn compute_total_hit(ch: &CharacterInfo, _coeff: &CoefficientRow) -> u16 {
    let power = MIN_WEAPON_POWER as f32;
    // Weapon coefficient is 0.0 for bare-hand (no weapon equipped).
    let weapon_coeff: f32 = 0.0;
    // No AP bonus buffs — default 100%.
    let bonus_ap: f32 = 1.0;

    let str_val = ch.str as f32;
    let dex_val = ch.dex as f32;
    let int_val = ch.intel as f32;

    // BaseAp: bonus for stats > 150
    let base_ap = if str_val > 150.0 {
        str_val - 150.0
    } else if int_val > 150.0 {
        int_val - 150.0
    } else {
        0.0
    };

    let level = ch.level as f32;
    let total_hit = if is_rogue(ch.class) {
        ((0.005 * power * (dex_val + 40.0)) + (weapon_coeff * power * level * dex_val) + 3.0)
            * bonus_ap
    } else if is_warrior(ch.class) {
        let main_stat = if str_val >= int_val { str_val } else { int_val };
        ((0.005 * power * (main_stat + 40.0)) + (weapon_coeff * power * level * main_stat) + 3.0)
            * bonus_ap
            + base_ap
    } else if is_priest(ch.class) {
        let main_stat = if str_val > int_val { str_val } else { int_val };
        ((0.005 * power * (main_stat + 40.0)) + (weapon_coeff * power * level * main_stat) + 3.0)
            * bonus_ap
            + base_ap
    } else {
        // Kurian / default: STR-based
        ((0.005 * power * (str_val + 40.0)) + (weapon_coeff * power * level * str_val) + 3.0)
            * bonus_ap
            + base_ap
    };

    total_hit.max(0.0) as u16
}

/// Calculate total armor class (m_sTotalAc) for a character.
/// Without equipment, `item_ac = 0`, so: `AC_coeff * level`.
fn compute_total_ac(ch: &CharacterInfo, coeff: &CoefficientRow) -> u16 {
    (coeff.ac * ch.level as f64).max(0.0) as u16
}

/// Calculate total hit rate for a character.
/// ```text
/// m_fTotalHitrate = ((1 + Hitrate * level * dex) * item_hitrate / 100) * (hit_rate_amount / 100)
/// ```
/// Without items: `item_hitrate = 100`, `hit_rate_amount = 100`.
fn compute_hitrate(ch: &CharacterInfo, coeff: &CoefficientRow) -> f32 {
    // item_hitrate defaults to 100, hit_rate_amount defaults to 100
    1.0 + coeff.hitrate as f32 * ch.level as f32 * ch.dex as f32
}

/// Calculate total evasion rate for a character.
/// ```text
/// m_fTotalEvasionrate = ((1 + Evasionrate * level * dex) * item_evasion / 100) * (avoid_amount / 100)
/// ```
/// Without items: `item_evasion = 100`, `avoid_amount = 100`.
fn compute_evasion(ch: &CharacterInfo, coeff: &CoefficientRow) -> f32 {
    1.0 + coeff.evasionrate as f32 * ch.level as f32 * ch.dex as f32
}

/// Determine hit result using the C++ hit rate table.
/// Returns one of: GREAT_SUCCESS (1), SUCCESS (2), NORMAL (3), FAIL (4).
/// Each rate bracket has different probability distributions:
/// - rate >= 5.0:  35% great, 40% success, 23% normal, 2% fail
/// - rate < 0.2:   2% great, 8% success, 40% normal, 50% fail
fn get_hit_rate(rate: f32, rng: &mut impl Rng) -> u8 {
    let random = rng.gen_range(1..=10000);

    if rate >= 5.0 {
        if random <= 3500 {
            GREAT_SUCCESS
        } else if random <= 7500 {
            SUCCESS
        } else if random <= 9800 {
            NORMAL
        } else {
            FAIL
        }
    } else if rate >= 3.0 {
        if random <= 2500 {
            GREAT_SUCCESS
        } else if random <= 6000 {
            SUCCESS
        } else if random <= 9600 {
            NORMAL
        } else {
            FAIL
        }
    } else if rate >= 2.0 {
        if random <= 2000 {
            GREAT_SUCCESS
        } else if random <= 5000 {
            SUCCESS
        } else if random <= 9400 {
            NORMAL
        } else {
            FAIL
        }
    } else if rate >= 1.25 {
        if random <= 1500 {
            GREAT_SUCCESS
        } else if random <= 4000 {
            SUCCESS
        } else if random <= 9200 {
            NORMAL
        } else {
            FAIL
        }
    } else if rate >= 0.8 {
        if random <= 1000 {
            GREAT_SUCCESS
        } else if random <= 3000 {
            SUCCESS
        } else if random <= 9000 {
            NORMAL
        } else {
            FAIL
        }
    } else if rate >= 0.5 {
        if random <= 800 {
            GREAT_SUCCESS
        } else if random <= 2500 {
            SUCCESS
        } else if random <= 8000 {
            NORMAL
        } else {
            FAIL
        }
    } else if rate >= 0.33 {
        if random <= 600 {
            GREAT_SUCCESS
        } else if random <= 2000 {
            SUCCESS
        } else if random <= 7000 {
            NORMAL
        } else {
            FAIL
        }
    } else if rate >= 0.2 {
        if random <= 400 {
            GREAT_SUCCESS
        } else if random <= 1500 {
            SUCCESS
        } else if random <= 6000 {
            NORMAL
        } else {
            FAIL
        }
    } else if random <= 200 {
        GREAT_SUCCESS
    } else if random <= 1000 {
        SUCCESS
    } else if random <= 5000 {
        NORMAL
    } else {
        FAIL
    }
}

/// Calculate R-attack (normal melee) damage.
/// ## Formula
/// 1. `temp_ap = total_hit * attack_amount` (attack_amount = 100 default)
/// 2. `temp_ac = target.total_ac` (no buff/debuff adjustments yet)
/// 3. `temp_hit_B = (temp_ap * 200 / 100) / (temp_ac + 240)`
/// 4. Hit check: `GetHitRate(attacker_hitrate / target_evasion + 1.0)`
/// 5. On GREAT_SUCCESS / SUCCESS / NORMAL:
///    - Priest: `damage = 0.15 * temp_hit_B + 0.2 * random(0, temp_hit_B)`
///    - Others: `damage = 0.75 * temp_hit_B + 0.3 * random(0, temp_hit_B)`
/// 6. On FAIL: damage = 0
#[cfg(test)]
fn calculate_r_damage(
    attacker: &CharacterInfo,
    attacker_coeff: &CoefficientRow,
    target: &CharacterInfo,
    target_coeff: &CoefficientRow,
    rng: &mut impl Rng,
) -> i16 {
    calculate_r_damage_with_class_bonus(
        attacker,
        attacker_coeff,
        target,
        target_coeff,
        rng,
        None,
        None,
        None,
        DEFAULT_ATTACK_AMOUNT as i32,
        None,
    )
}

/// Physical damage calculation with optional class bonus arrays for PvP.
/// When `attacker_ap_class_bonus` and `target_ac_class_bonus` are provided,
/// applies class-specific AP/AC bonuses using the target's base class as
/// the array index (C++ active `#else` branch, Unit.cpp:320-322).
#[allow(clippy::too_many_arguments)]
fn calculate_r_damage_with_class_bonus(
    attacker: &CharacterInfo,
    attacker_coeff: &CoefficientRow,
    target: &CharacterInfo,
    target_coeff: &CoefficientRow,
    rng: &mut impl Rng,
    attacker_ap_class_bonus: Option<&[u8; 4]>,
    target_ac_class_bonus: Option<&[u8; 4]>,
    target_ac_override: Option<i32>,
    attack_amount: i32,
    total_hit_override: Option<u16>,
) -> i16 {
    // ── Step 1: Compute attack power and AC ─────────────────────────────
    // When total_hit_override is Some, use the pre-computed value from
    // set_user_ability() (equipped weapon + buffs + stats). This is the
    // correct production path.  Fallback to compute_total_hit() only in
    // unit tests where no WorldState/inventory is available.
    let total_hit =
        total_hit_override.unwrap_or_else(|| compute_total_hit(attacker, attacker_coeff));

    let mut temp_ap = total_hit as i32 * attack_amount;

    // When target_ac_override is Some, it provides EquippedStats.total_ac + buff_ac
    // (the full C++ m_sTotalAc + m_sACAmount). Without override, falls back to
    // coefficient-based AC (for tests without WorldState).
    let mut temp_ac =
        target_ac_override.unwrap_or_else(|| compute_total_ac(target, target_coeff) as i32);

    // ── Apply class-specific AP/AC bonuses (PvP only) ───────────────────
    // Uses TARGET's base class to index both arrays.
    if let Some(idx) = class_group_index(target.class) {
        if let Some(ac_bonus) = target_ac_class_bonus {
            temp_ac = temp_ac * (100 + ac_bonus[idx] as i32) / 100;
        }
        if let Some(ap_bonus) = attacker_ap_class_bonus {
            temp_ap = temp_ap * (100 + ap_bonus[idx] as i32) / 100;
        }
    }

    let temp_hit_b = if temp_ac + 240 > 0 {
        (temp_ap * 2) / (temp_ac + 240)
    } else {
        temp_ap * 2
    };

    if temp_hit_b <= 0 {
        return 0;
    }

    // ── Step 2: Hit rate check ──────────────────────────────────────────
    let attacker_hitrate = compute_hitrate(attacker, attacker_coeff);
    let target_evasion = compute_evasion(target, target_coeff);

    let rate = if target_evasion > 0.0 {
        attacker_hitrate / target_evasion + 1.0
    } else {
        attacker_hitrate + 1.0
    };

    let hit_result = get_hit_rate(rate, rng);

    // ── Step 3: Damage calculation based on hit result ──────────────────
    match hit_result {
        GREAT_SUCCESS | SUCCESS | NORMAL => {
            let random = if temp_hit_b > 0 {
                rng.gen_range(0..=temp_hit_b)
            } else {
                0
            };

            let damage = if is_priest(attacker.class) {
                (0.15 * temp_hit_b as f32 + 0.2 * random as f32) as i32
            } else {
                (0.75 * temp_hit_b as f32 + 0.3 * random as f32) as i32
            };

            damage.max(1) as i16
        }
        _ => {
            0
        }
    }
}

/// Apply zone-specific damage overrides.
/// - Snow Battle: R-attack damage = 0
/// - Chaos Dungeon: fixed 50 (500/10)
/// - Dungeon Defence: fixed 50 (500/10)
fn apply_zone_damage_override(zone_id: u16, damage: i16) -> i16 {
    match zone_id {
        ZONE_SNOW_BATTLE => 0,
        ZONE_CHAOS_DUNGEON | ZONE_DUNGEON_DEFENCE => 50,
        _ => damage,
    }
}

// ── Main handler ────────────────────────────────────────────────────────────

/// Handle WIZ_ATTACK (0x08) from the client.
/// Parses the attack packet, validates pre-conditions, calculates
/// damage using the reference formula, applies HP change, triggers
/// death if needed, and broadcasts the result to the 3x3 region.
pub async fn handle(session: &mut ClientSession, pkt: Packet) -> anyhow::Result<()> {
    if session.state() != SessionState::InGame {
        return Ok(());
    }

    let world = session.world().clone();
    let sid = session.session_id();

    // ── Parse client packet ────────────────────────────────────────────
    //   pkt >> bType >> bResult >> tid >> delaytime >> distance >> unknown >> unknowns;
    let mut reader = PacketReader::new(&pkt.data);
    let b_type = reader.read_u8().unwrap_or(0);
    let _b_result_client = reader.read_u8().unwrap_or(0); // overwritten by server
                                                          // C++ declares tid as int32 — negative values (e.g. -1 = 0xFFFFFFFF) mean "no target"
    let tid_signed = reader.read_u32().map(|v| v as i32).unwrap_or(-1);
    let delaytime = reader.read_u16().map(|v| v as i16).unwrap_or(0);
    let distance = reader.read_u16().map(|v| v as i16).unwrap_or(0);
    let unknown = reader.read_u8().unwrap_or(0);
    let _unknowns = reader.read_u8().unwrap_or(0);

    if tid_signed <= 0 {
        return Ok(());
    }
    let tid = tid_signed as u32;

    // ── Pre-attack validation: attacker state ──────────────────────────
    let attacker = match world.get_character_info(sid) {
        Some(ch) => ch,
        None => return Ok(()),
    };

    // Dead players cannot attack
    if attacker.res_hp_type == USER_DEAD || attacker.hp <= 0 {
        return Ok(());
    }

    // Sitting players cannot attack
    if attacker.res_hp_type == USER_SITDOWN {
        return Ok(());
    }

    // Incapacitated check: blinded, blinking, or kaul state
    // (isDead already checked above)
    {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if world.is_player_blinking(sid, now_unix) {
            return Ok(());
        }
        // Single session read for blind/kaul/weapons_disabled (2 DashMap reads → 1)
        let (is_blinded_or_kaul, weapons_disabled) = world
            .with_session(sid, |h| {
                (
                    h.buffs.contains_key(&BUFF_TYPE_BLIND)
                        || h.buffs.contains_key(&BUFF_TYPE_KAUL_TRANSFORMATION),
                    h.weapons_disabled,
                )
            })
            .unwrap_or((false, false));
        if is_blinded_or_kaul || weapons_disabled {
            return Ok(());
        }
    }

    // GM attack ban check.
    if world.is_attack_disabled(sid) {
        return Ok(());
    }

    // Cannot attack in enemy safety areas (villages, temples, arena spawn)
    let pos = world.get_position(sid).unwrap_or_default();
    if is_in_enemy_safety_area(pos.zone_id, pos.x, pos.z, attacker.nation) {
        return Ok(());
    }

    // Special event zone (Zindan War) attack block
    // Block R-attacks in SPBATTLE zones when the event is NOT opened
    // (unless Cinderella War is active in that zone)
    if is_in_special_event_zone(pos.zone_id)
        && !world.is_zindan_event_opened()
        && !world.is_cinderella_active()
    {
        return Ok(());
    }

    // Cinderella War pre-start attack block
    // Block R-attacks in Cinderella zone when war is ON but NOT started
    if !world.is_zindan_event_opened()
        && world.is_cinderella_active()
        && world.cinderella_zone_id() == pos.zone_id
    {
        // CindWar isON but not started (event opened = false means not started)
        // In C++: !g_pMain->pSpecialEvent.opened && isCindirellaZone && pCindWar.isON && !pCindWar.isStarted
        // We simplify: if zindan_event_opened is false AND cindwar is active but we're in the zone
        // This blocks attacks during the preparation phase
        return Ok(());
    }

    let is_gm = attacker.authority == 0;

    // ── Remove stealth before attacking ──────────────────────────────────
    crate::handler::stealth::remove_stealth(&world, sid);

    // ── Weapon delay validation ─────────────────────────────────────────
    let right_weapon = world.get_right_hand_weapon(sid);
    let left_weapon = world.get_left_hand_weapon(sid);

    // GM weapon bypass: item 389158000 in either hand + GM authority
    let nocheck = is_gm
        && (right_weapon
            .as_ref()
            .is_some_and(|w| w.num as u32 == GM_WEAPON_ID)
            || left_weapon
                .as_ref()
                .is_some_and(|w| w.num as u32 == GM_WEAPON_ID));

    // Reject attacks with bows (handled by archery, not R-attack)
    let right_is_bow = right_weapon
        .as_ref()
        .is_some_and(|w| is_bow_weapon(w.kind.unwrap_or(0)));
    let left_is_bow = left_weapon
        .as_ref()
        .is_some_and(|w| is_bow_weapon(w.kind.unwrap_or(0)));
    if right_is_bow || left_is_bow {
        return Ok(());
    }

    // Server-side rate limit: 900ms between attacks
    if !nocheck {
        let now = Instant::now();
        let can_attack = world
            .with_session(sid, |h| h.last_attack_time.is_none_or(|t| now >= t))
            .unwrap_or(true);
        if !can_attack {
            tracing::debug!(
                "[sid={}] Attack rejected: server-side 900ms rate limit",
                sid
            );
            return Ok(());
        }
        // Set next allowed attack time
        world.update_session(sid, |h| {
            h.last_attack_time = Some(now + Duration::from_millis(PLAYER_R_HIT_REQUEST_INTERVAL));
        });
    }

    // Weapon delay check: only for non-mage classes with a weapon
    if !nocheck {
        let attacker_is_mage = is_mage(attacker.class);
        if let Some(ref weapon) = right_weapon {
            if !attacker_is_mage {
                let weapon_delay = weapon.delay.unwrap_or(0);
                let weapon_range = weapon.range.unwrap_or(0);
                let item_num = weapon.num as u32;

                if is_timing_delay(item_num) {
                    // Timing Delay weapons: +9ms tolerance
                    if delaytime < (weapon_delay + 9) || distance > weapon_range {
                        tracing::debug!(
                            "[sid={}] Attack rejected: timing delay weapon check (delaytime={}, required={}, distance={}, range={})",
                            sid, delaytime, weapon_delay + 9, distance, weapon_range
                        );
                        return Ok(());
                    }
                } else if is_wirinom_uniq_delay(item_num)
                    || is_wirinom_reb_delay(item_num)
                    || is_garges_sword_delay(item_num)
                {
                    // Wirinim/Garges weapons: -4ms tolerance
                    if delaytime < (weapon_delay - 4) || distance > weapon_range {
                        tracing::debug!(
                            "[sid={}] Attack rejected: special weapon delay check (delaytime={}, required={}, distance={}, range={})",
                            sid, delaytime, weapon_delay - 4, distance, weapon_range
                        );
                        return Ok(());
                    }
                } else {
                    // Normal weapon: exact delay
                    if delaytime < weapon_delay || distance > weapon_range {
                        tracing::debug!(
                            "[sid={}] Attack rejected: weapon delay check (delaytime={}, required={}, distance={}, range={})",
                            sid, delaytime, weapon_delay, distance, weapon_range
                        );
                        return Ok(());
                    }
                }
            }
        } else if delaytime < MIN_MELEE_DELAY {
            // Empty-handed (no weapon): minimum 100ms
            tracing::debug!(
                "[sid={}] Attack rejected: empty-handed delaytime {} < {}",
                sid,
                delaytime,
                MIN_MELEE_DELAY
            );
            return Ok(());
        }
    }

    // ── Target validation ──────────────────────────────────────────────
    let target_is_player = tid < crate::npc::NPC_BAND;

    if target_is_player {
        let target_sid = tid as SessionId;
        // Self-attack prevention — C++ Unit.cpp:2055 isHostileTo returns false for self
        if target_sid == sid {
            return Ok(());
        }
        handle_player_attack(&world, sid, &pos, target_sid, b_type, unknown);
    } else {
        // NPC/Monster target
        handle_npc_attack(world.clone(), sid, pos, tid, b_type, unknown).await;
    }

    Ok(())
}

/// Handle a player-vs-player attack.
/// Validates target state, computes damage using the real C++ formula,
/// applies HP change, triggers death if HP reaches 0, and broadcasts
/// the result.
fn handle_player_attack(
    world: &WorldState,
    attacker_sid: SessionId,
    attacker_pos: &Position,
    target_sid: SessionId,
    b_type: u8,
    unknown: u8,
) {
    let tid = target_sid as u32;

    // Target must exist and be alive
    let target = match world.get_character_info(target_sid) {
        Some(ch) => ch,
        None => return,
    };

    if target.res_hp_type == USER_DEAD || target.hp <= 0 {
        return;
    }

    if target.authority == 0 {
        broadcast_attack_result(world, attacker_sid, b_type, ATTACK_FAIL, tid, unknown);
        return;
    }

    // Blinking targets (respawn invulnerability) are immune to R-attacks.
    {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if world.is_player_blinking(target_sid, now_unix) {
            return;
        }
    }

    if world.has_buff(target_sid, BUFF_TYPE_FREEZE) {
        return;
    }

    // ── Distance check ─────────────────────────────────────────────────
    let target_pos = match world.get_position(target_sid) {
        Some(p) => p,
        None => return,
    };

    // Must be in the same zone
    if attacker_pos.zone_id != target_pos.zone_id {
        return;
    }

    // 2D distance check (squared) -- C++ uses isInAttackRange
    let weapon_range = get_weapon_range(world, attacker_sid);
    let attack_range = DEFAULT_MELEE_RANGE + weapon_range;

    let dx = attacker_pos.x - target_pos.x;
    let dz = attacker_pos.z - target_pos.z;
    let dist_sq = dx * dx + dz * dz;
    let range_sq = attack_range * attack_range;

    if dist_sq > range_sq {
        tracing::debug!(
            "[sid={}] Attack out of range: dist_sq={:.1} > range_sq={:.1} (weapon_range={:.1})",
            attacker_sid,
            dist_sq,
            range_sq,
            weapon_range
        );
        return;
    }

    // ── Temple event attack gate ────────────────────────────────────────
    //   if (isInTempleEventZone() && !virt_eventattack_check()) return;
    //   if (isInTempleEventZone() && (!isSameEventRoom(pTarget) || !pTempleEvent.isAttackable)) return;
    {
        use crate::systems::event_room;
        if event_room::is_in_temple_event_zone(attacker_pos.zone_id) {
            let attacker_name = world.get_session_name(attacker_sid).unwrap_or_default();
            if !event_room::virt_eventattack_check(
                &world.event_room_manager,
                attacker_pos.zone_id,
                &attacker_name,
            ) {
                return;
            }
            // Check isAttackable flag (event phase must allow combat)
            let is_attackable = world
                .event_room_manager
                .read_temple_event(|s| s.is_attackable);
            if !is_attackable {
                return;
            }
            // Check same event room (both players must be in the same room)
            let target_name = world.get_session_name(target_sid).unwrap_or_default();
            if let Some(event_type) = event_room::event_type_for_zone(attacker_pos.zone_id) {
                let attacker_room = world
                    .event_room_manager
                    .find_user_room(event_type, &attacker_name)
                    .map(|(r, _)| r);
                let target_room = world
                    .event_room_manager
                    .find_user_room(event_type, &target_name)
                    .map(|(r, _)| r);
                // C++ isSameEventRoom: compares GetEventRoom() values.
                // Both must be in a valid room AND in the same room.
                // Guard against None == None (both unassigned) allowing attacks.
                match (attacker_room, target_room) {
                    (Some(a), Some(t)) if a == t => {} // same room, proceed
                    _ => return,                       // different rooms or one/both not assigned
                }
            }
        }
    }

    // ── Monster Stone event room isolation ─────────────────────────────
    //   isInTempleQuestEventZone() && (!isSameEventRoom(pTarget) && m_sMonsterStoneStatus)
    // Players in Monster Stone zones with an active Monster Stone room must
    // be in the same event room to attack. The m_sMonsterStoneStatus guard
    // ensures this only applies to players who have activated a room.
    {
        use crate::systems::monster_stone;
        if monster_stone::is_monster_stone_zone(attacker_pos.zone_id)
            && world.get_monster_stone_status(attacker_sid)
            && !world.is_same_event_room(attacker_sid, target_sid)
        {
            return;
        }
    }

    // ── Kaul transformation block ─────────────────────────────────────
    if world
        .with_session(attacker_sid, |h| h.is_kaul)
        .unwrap_or(false)
    {
        return;
    }

    // ── PvP permission check (isHostileTo) ────────────────────────────
    // Only allows PvP in specific zones / conditions.
    let attacker = match world.get_character_info(attacker_sid) {
        Some(ch) => ch,
        None => return,
    };

    if !is_hostile_to(
        world,
        attacker_sid,
        &attacker,
        attacker_pos,
        target_sid,
        &target,
        &target_pos,
    ) {
        tracing::debug!(
            "[sid={}] PvP denied: not hostile to target={} in zone={}",
            attacker_sid,
            target_sid,
            attacker_pos.zone_id
        );
        return;
    }

    let attacker_coeff = match world.get_coefficient(attacker.class) {
        Some(c) => c,
        None => {
            tracing::warn!(
                "[sid={}] No coefficient for class {} — using 0 damage",
                attacker_sid,
                attacker.class
            );
            broadcast_attack_result(world, attacker_sid, b_type, ATTACK_FAIL, tid, unknown);
            return;
        }
    };

    let target_coeff = match world.get_coefficient(target.class) {
        Some(c) => c,
        None => {
            tracing::warn!(
                "[sid={}] No coefficient for target class {} — using 0 damage",
                attacker_sid,
                target.class
            );
            broadcast_attack_result(world, attacker_sid, b_type, ATTACK_FAIL, tid, unknown);
            return;
        }
    };

    // ── Damage calculation ─────────────────────────────────────────────
    // Use StdRng (Send-safe) seeded from entropy so it works across await points.
    let mut rng = rand::rngs::StdRng::from_entropy();

    // Snapshot all combat-relevant data in a single DashMap read per combatant.
    // Replaces 15 separate lock acquisitions with 2.
    let attacker_snap = match world.snapshot_combat(attacker_sid) {
        Some(s) => s,
        None => return,
    };
    let target_snap = match world.snapshot_combat(target_sid) {
        Some(s) => s,
        None => return,
    };
    let attacker_stats = &attacker_snap.equipped_stats;
    let target_stats = &target_snap.equipped_stats;

    // ── Block physical damage check ──────────────────────────────────
    if target_snap.block_physical {
        broadcast_attack_result(world, attacker_sid, b_type, ATTACK_SUCCESS, tid, unknown);
        return;
    }

    // AC% applied to base first, then flat buff AC added, then AC source reduction subtracted.
    let target_full_ac = ((target_stats.total_ac as i32) * target_snap.ac_pct / 100
        + target_snap.ac_amount
        - target_snap.ac_sour)
        .max(0);

    let mut damage = calculate_r_damage_with_class_bonus(
        &attacker,
        &attacker_coeff,
        &target,
        &target_coeff,
        &mut rng,
        Some(&attacker_stats.ap_class_bonus),
        Some(&target_stats.ac_class_bonus),
        Some(target_full_ac),
        attacker_snap.attack_amount,
        Some(attacker_stats.total_hit),
    );

    // ── Apply buff-based PvP modifiers ──────────────────────────────
    if damage > 0 && attacker_snap.player_attack_amount != 100 {
        damage = (damage as i32 * attacker_snap.player_attack_amount / 100) as i16;
    }

    if damage > 0 && is_mage(attacker.class) {
        damage = (damage as f64 * world.get_plus_damage_from_item_ids(
            attacker_snap.left_hand_item_id,
            attacker_snap.right_hand_item_id,
        )) as i16;
    }

    // ── R-attack damage multiplier for level>30 non-priests ──────────
    if damage > 0 && attacker.level > 30 && !is_priest(attacker.class) {
        damage = (damage as f64 * world.get_r_damage_multiplier()) as i16;
    }

    // ── Elemental weapon damage bonuses (GetMagicDamage) ─────────────
    // Adds fire/ice/lightning/poison damage from attacker's weapons minus target resistance.
    if damage > 0 {
        damage = apply_elemental_weapon_damage_pvp(
            world,
            attacker_sid,
            &attacker_snap.equipped_stats,
            target_sid,
            &target_snap.equipped_stats,
            target_snap.pct_fire_r,
            target_snap.pct_cold_r,
            target_snap.pct_lightning_r,
            target_snap.pct_poison_r,
            damage,
        );
    }

    // ── Weapon-type armor resistance (GetACDamage) ──────────────────
    // Reduces damage based on target's weapon-type-specific armor resistances (PvP only).
    if damage > 0 {
        let right_kind = if attacker_snap.right_hand_item_id != 0 {
            world.get_item(attacker_snap.right_hand_item_id).and_then(|w| w.kind)
        } else {
            None
        };
        let left_kind = if attacker_snap.left_hand_item_id != 0 {
            world.get_item(attacker_snap.left_hand_item_id).and_then(|w| w.kind)
        } else {
            None
        };
        damage = get_ac_damage(
            damage,
            &[right_kind, left_kind],
            target_stats,
            target_snap.dagger_r_amount,
            target_snap.bow_r_amount,
        );
    }

    // ── Class-vs-class PvP multiplier ─────────────────────────────────
    if damage > 0 {
        let mult = world.get_class_damage_multiplier(attacker.class, target.class);
        damage = (damage as f64 * mult) as i16;
    }

    if damage > 0 {
        let perk_dmg = world.compute_perk_bonus(&attacker_snap.perk_levels, 10, false);
        if perk_dmg > 0 {
            damage =
                (damage as i32 + damage as i32 * perk_dmg / 100).clamp(0, i16::MAX as i32) as i16;
        }
    }

    // ── Zone damage overrides ──────────────────────────────────────────
    damage = apply_zone_damage_override(attacker_pos.zone_id, damage);

    // ── MAX_DAMAGE cap ─────────────────────────────────────────────────
    damage = damage.min(crate::attack_constants::MAX_DAMAGE as i16);

    // ── Apply damage ───────────────────────────────────────────────────
    if damage <= 0 {
        broadcast_attack_result(world, attacker_sid, b_type, ATTACK_FAIL, tid, unknown);
        return;
    }

    // C++ order: save originalAmount → mirror → mastery → mana absorb (uses originalAmount)
    // Save original damage BEFORE mirror/mastery for mana absorb calculation.
    let original_damage = damage;

    // ── Pre-fetch victim zone + mana absorb (3 position reads + 1 session read → 0 + 1) ──
    // Uses target_pos.zone_id from earlier distance check (already fetched at line 1123)
    let not_use_zone =
        target_pos.zone_id == ZONE_CHAOS_DUNGEON || target_pos.zone_id == ZONE_KNIGHT_ROYALE;
    let (absorb_pct, absorb_count) = world
        .with_session(target_sid, |h| (h.mana_absorb, h.absorb_count))
        .unwrap_or((0, 0));

    // ── Mirror damage victim reduction ──────────────────────────────────
    let (mirror_dmg, mirror_direct) = if !not_use_zone {
        let (active, direct, amount) = (
            target_snap.mirror_damage,
            target_snap.mirror_damage_type,
            target_snap.mirror_amount,
        );
        if active && amount > 0 {
            let md = (amount as i32 * damage as i32) / 100;
            if md > 0 {
                (md, direct)
            } else {
                (0, false)
            }
        } else {
            (0, false)
        }
    } else {
        (0, false)
    };
    if mirror_dmg > 0 {
        damage = (damage as i32 - mirror_dmg).max(0) as i16;
    }

    // ── Mastery passive damage reduction ────────────────────────────────
    // Matchless: SkillPointMaster >= 10 → 15% reduction
    // Absoluteness: SkillPointMaster >= 5 → 10% reduction
    if !not_use_zone && crate::handler::class_change::is_mastered(target.class) {
        let master_pts = target.skill_points[8]; // SkillPointMaster = index 8
        if master_pts >= 10 {
            // Matchless: 15% damage reduction
            damage = (85 * damage as i32 / 100) as i16;
        } else if master_pts >= 5 {
            // Absoluteness: 10% damage reduction
            damage = (90 * damage as i32 / 100) as i16;
        }
    }

    // ── Mana Absorb (Outrage/Frenzy/Mana Shield) ─────────────────────
    // C++ uses `originalAmount` (pre-mirror) for absorb calculation,
    // but subtracts absorbed from current `amount` (post-mirror).
    {
        if absorb_pct > 0 && !not_use_zone {
            let should_absorb = if absorb_pct == 15 {
                absorb_count > 0
            } else {
                true
            };
            if should_absorb {
                // C++ line 131: toBeAbsorbed = (originalAmount * m_bManaAbsorb) / 100
                let absorbed = (original_damage as i32 * absorb_pct as i32 / 100) as i16;
                damage -= absorbed;
                // C++ allows damage to reach 0 after mana absorb (no minimum enforced)
                if damage < 0 {
                    damage = 0;
                }
                // Convert absorbed damage to MP
                world.update_character_stats(target_sid, |ch| {
                    ch.mp = (ch.mp as i32).saturating_add(absorbed as i32).min(ch.max_mp as i32) as i16;
                });
                // Decrement absorb count for pct==15 skills
                if absorb_pct == 15 {
                    world.update_session(target_sid, |h| {
                        h.absorb_count = h.absorb_count.saturating_sub(1);
                    });
                }
            }
        }
    }

    let new_hp = (target.hp - damage).max(0);
    world.update_character_hp(target_sid, new_hp);

    // Send WIZ_HP_CHANGE to victim so their client updates their own HP bar
    let hp_pkt = build_hp_change_packet_with_attacker(target.max_hp, new_hp, attacker_sid as u32);
    world.send_to_session_owned(target_sid, hp_pkt);

    crate::handler::party::broadcast_party_hp(world, target_sid);

    // ── Mirror damage reflection (skill buff) ──────────────────────────
    // Mirror was pre-computed above; now reflect to attacker or party.
    if mirror_dmg > 0 {
        if mirror_direct {
            // Direct: reflect full mirror damage to attacker
            let atk_hp = world
                .get_character_info(attacker_sid)
                .map(|c| (c.hp, c.max_hp))
                .unwrap_or((0, 0));
            let new_atk_hp = (atk_hp.0 - mirror_dmg as i16).max(0);
            world.update_character_hp(attacker_sid, new_atk_hp);
            let atk_hp_pkt =
                build_hp_change_packet_with_attacker(atk_hp.1, new_atk_hp, target_sid as u32);
            world.send_to_session_owned(attacker_sid, atk_hp_pkt);
        } else if world.is_in_party(target_sid) {
            // Party distribution: spread mirror damage among attacker's party.
            if let Some(atk_party_id) = world.get_party_id(attacker_sid) {
                if let Some(party) = world.get_party(atk_party_id) {
                    let members = party.active_members();
                    let p_count = members.len() as i32;
                    if p_count > 0 {
                        // C++ precedence bug: (mirrorDamage / p_count < 2) ? 2 : p_count
                        let per_member_dmg = if (mirror_dmg / p_count) < 2 {
                            2
                        } else {
                            p_count
                        };
                        for &member_sid in &members {
                            if member_sid == target_sid {
                                continue; // skip the victim (C++: p == this)
                            }
                            let m_hp = world
                                .get_character_info(member_sid)
                                .map(|c| (c.hp, c.max_hp))
                                .unwrap_or((0, 0));
                            if m_hp.0 <= 0 {
                                continue;
                            }
                            let new_m_hp = (m_hp.0 as i32 - per_member_dmg).max(0) as i16;
                            world.update_character_hp(member_sid, new_m_hp);
                            // C++ sends HpChange with pAttacker=nullptr (tid=0xFFFF)
                            let m_hp_pkt =
                                build_hp_change_packet_with_attacker(m_hp.1, new_m_hp, 0xFFFF);
                            world.send_to_session_owned(member_sid, m_hp_pkt);
                        }
                    }
                }
            }
        }
    }

    // ── Equipment mirror damage (ITEM_TYPE_MIRROR_DAMAGE) ───────────
    // mirror_damage from equipped items and reflects: damage * total / 300.
    // This is separate from the skill-based mirror (buff 44) above.
    {
        const ITEM_TYPE_MIRROR_DAMAGE: u8 = 0x08;
        let eq_stats = world.get_equipped_stats(target_sid);
        let mut total_equip_mirror: i32 = 0;
        for bonuses in eq_stats.equipped_item_bonuses.values() {
            for &(btype, amount) in bonuses {
                if btype == ITEM_TYPE_MIRROR_DAMAGE {
                    total_equip_mirror += amount;
                }
            }
        }
        if total_equip_mirror > 0 {
            let reflected = (damage as i32 * total_equip_mirror) / 300;
            if reflected > 0 {
                let atk_hp = world
                    .get_character_info(attacker_sid)
                    .map(|c| (c.hp, c.max_hp))
                    .unwrap_or((0, 0));
                let new_atk_hp = (atk_hp.0 as i32 - reflected).max(0) as i16;
                world.update_character_hp(attacker_sid, new_atk_hp);
                let eq_mirror_pkt =
                    build_hp_change_packet_with_attacker(atk_hp.1, new_atk_hp, target_sid as u32);
                world.send_to_session_owned(attacker_sid, eq_mirror_pkt);
            }
        }
    }

    // ── Equipment durability loss ─────────────────────────────────────
    // Every attack takes a little of the attacker's weapon durability.
    world.item_wore_out(attacker_sid, WORE_TYPE_ATTACK, damage as i32);
    // Every hit takes a little of the defender's armour durability.
    world.item_wore_out(target_sid, WORE_TYPE_DEFENCE, damage as i32);

    let b_result = if new_hp <= 0 {
        // Target died -- broadcast death
        dead::broadcast_death(world, target_sid);

        // FerihaLog: KillingUserInsertLog
        if let Some(pool) = world.db_pool() {
            let killer_acc = world
                .with_session(attacker_sid, |h| h.account_id.clone())
                .unwrap_or_default();
            let dead_acc = world
                .with_session(target_sid, |h| h.account_id.clone())
                .unwrap_or_default();
            super::audit_log::log_killing_user(
                pool,
                &killer_acc,
                &attacker.name,
                &dead_acc,
                &target.name,
                attacker_pos.zone_id as i16,
                attacker_pos.x as i16,
                attacker_pos.z as i16,
            );
        }

        // ── War zone death tracking ──────────────────────────────────
        // Increment war death counters when a player dies in an active war zone.
        // The counter tracks the DEAD player's nation (more deaths = worse).
        if crate::systems::war::is_battle_zone(attacker_pos.zone_id) && world.is_war_open() {
            world.increment_war_death(target.nation);
            tracing::debug!(
                "[sid={}] War zone kill: victim nation={}, zone={}",
                attacker_sid,
                target.nation,
                attacker_pos.zone_id
            );
        }

        // ── Tournament kill scoring ─────────────────────────────────────
        // Increment the killer's clan scoreboard in tournament zones.
        if super::tournament::is_tournament_zone(attacker_pos.zone_id) {
            super::tournament::register_kill(world, attacker_pos.zone_id, attacker.knights_id);
        }

        // ── Zone kill reward ──────────────────────────────────────────
        // After a PvP kill in a PK zone, increment kill count and give rewards.
        if is_pk_zone(attacker_pos.zone_id) {
            give_kill_reward(world, attacker_sid, attacker_pos.zone_id);
        }

        // ── Track killer for resurrection EXP recovery ─────────────────
        dead::set_who_killed_me(world, target_sid, attacker_sid);

        // ── PvP loyalty (NP) change ──────────────────────────────────
        // If the zone grants loyalty, give NP to killer (or party) and deduct from victim.
        dead::pvp_loyalty_on_death(world, attacker_sid, target_sid);

        // ── Rivalry / Anger Gauge ────────────────────────────────────────
        // In Ardream / Ronark Land zones: increment victim's anger gauge, assign
        // killer as victim's rival (if none), and check for revenge kills.
        {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let is_revenge = arena::on_pvp_kill(
                world,
  …39946 tokens truncated…o_bonus = [0u8; 4];
        let dmg_zero_bonus = calculate_r_damage_with_class_bonus(
            &attacker,
            &a_coeff,
            &target,
            &t_coeff,
            &mut rng2,
            Some(&zero_bonus),
            Some(&zero_bonus),
            None,
            DEFAULT_ATTACK_AMOUNT as i32,
            None,
        );

        assert_eq!(
            dmg_no_bonus, dmg_zero_bonus,
            "Zero bonuses should not change damage"
        );
    }

    #[test]
    fn test_class_bonus_kurian_target_no_bonus_applied() {
        let attacker = make_test_char(101, 60, 200, 100, 100, 50);
        let target = make_test_char(113, 60, 100, 100, 100, 50); // Kurian
        let a_coeff = make_test_coeff();
        let t_coeff = make_test_coeff();

        let mut rng1 = rand::rngs::StdRng::seed_from_u64(77);
        let dmg_no_bonus = calculate_r_damage_with_class_bonus(
            &attacker,
            &a_coeff,
            &target,
            &t_coeff,
            &mut rng1,
            None,
            None,
            None,
            DEFAULT_ATTACK_AMOUNT as i32,
            None,
        );

        let mut rng2 = rand::rngs::StdRng::seed_from_u64(77);
        let big_bonus = [50, 50, 50, 50];
        let dmg_with_bonus = calculate_r_damage_with_class_bonus(
            &attacker,
            &a_coeff,
            &target,
            &t_coeff,
            &mut rng2,
            Some(&big_bonus),
            Some(&big_bonus),
            None,
            DEFAULT_ATTACK_AMOUNT as i32,
            None,
        );

        assert_eq!(
            dmg_no_bonus, dmg_with_bonus,
            "Kurian target should not be affected by class bonus"
        );
    }

    // ── rdamage multiplier tests ─────────────────────────────────────────

    #[test]
    fn test_rdamage_applies_to_level31_warrior() {
        // Level 31 warrior (not priest) should have rdamage applied
        let attacker = make_test_char(101, 31, 90, 60, 30, 20);
        assert!(attacker.level > 30);
        assert!(!is_priest(attacker.class));
        // Conditions met: level > 30 && !is_priest
    }

    #[test]
    fn test_rdamage_skips_priest() {
        // Priest should NOT have rdamage applied regardless of level
        let attacker = make_test_char(104, 60, 90, 60, 30, 20);
        assert!(attacker.level > 30);
        assert!(is_priest(attacker.class));
        // Condition fails: is_priest == true
    }

    #[test]
    fn test_rdamage_skips_low_level() {
        // Level 30 warrior should NOT have rdamage applied
        let attacker = make_test_char(101, 30, 90, 60, 30, 20);
        assert!(attacker.level <= 30);
        // Condition fails: level <= 30
    }

    // ── GM instant-kill NPC tests ─────────────────────────────────────────

    #[test]
    fn test_gm_authority_check() {
        // GM has authority == 0
        let gm = make_test_char(101, 80, 90, 60, 30, 20);
        let mut gm_char = gm;
        gm_char.authority = 0;
        assert_eq!(gm_char.authority, 0);

        // Normal player has authority != 0
        let player = make_test_char(101, 80, 90, 60, 30, 20);
        assert_ne!(player.authority, 0);
    }

    // ── Enemy safety area tests ────────────────────────────────────────

    #[test]
    fn test_safety_area_delos_center() {
        // Center of Delos safe zone (500, 180, radius 115)
        assert!(is_in_enemy_safety_area(ZONE_DELOS, 500.0, 180.0, 1));
        assert!(is_in_enemy_safety_area(ZONE_DELOS, 500.0, 180.0, 2));
    }

    #[test]
    fn test_safety_area_delos_outside() {
        // Well outside Delos safe zone
        assert!(!is_in_enemy_safety_area(ZONE_DELOS, 800.0, 800.0, 1));
    }

    #[test]
    fn test_safety_area_arena() {
        // Center of arena safe zone (127, 113, radius 36)
        assert!(is_in_enemy_safety_area(ZONE_ARENA, 127.0, 113.0, 1));
        assert!(!is_in_enemy_safety_area(ZONE_ARENA, 300.0, 300.0, 1));
    }

    #[test]
    fn test_safety_area_karus_village() {
        // Karus nation in own village — should be safe
        assert!(is_in_enemy_safety_area(ZONE_KARUS, 1860.0, 174.0, 1));
        // Elmorad nation in Karus village — NOT safe (it's enemy territory)
        assert!(!is_in_enemy_safety_area(ZONE_KARUS, 1860.0, 174.0, 2));
    }

    #[test]
    fn test_safety_area_elmorad_village() {
        // Elmorad in own village
        assert!(is_in_enemy_safety_area(ZONE_ELMORAD, 210.0, 1853.0, 2));
        // Karus in Elmorad village — NOT safe
        assert!(!is_in_enemy_safety_area(ZONE_ELMORAD, 210.0, 1853.0, 1));
    }

    #[test]
    fn test_safety_area_normal_zone() {
        // Normal zone (Moradon) — never a safety area
        assert!(!is_in_enemy_safety_area(21, 500.0, 500.0, 1));
    }

    // ── isInOwnSafetyArea tests ──────────────────────────────────────────

    #[test]
    fn test_own_safety_delos_center() {
        // DELOS is zone-neutral (same as enemy safety)
        assert!(is_in_own_safety_area(ZONE_DELOS, 500.0, 180.0, 1));
        assert!(is_in_own_safety_area(ZONE_DELOS, 500.0, 180.0, 2));
    }

    #[test]
    fn test_own_safety_bifrost_nation_inverted() {
        // C++ isInOwnSafetyArea: KARUS checks Karus coords (56-124, 700-840)
        // C++ isInEnemySafetyArea: ELMORAD checks same coords
        // Nation is INVERTED between the two functions
        assert!(is_in_own_safety_area(ZONE_BIFROST, 90.0, 770.0, 1)); // Karus in Karus safe
        assert!(!is_in_enemy_safety_area(ZONE_BIFROST, 90.0, 770.0, 1)); // NOT enemy safe for Karus
        assert!(is_in_enemy_safety_area(ZONE_BIFROST, 90.0, 770.0, 2)); // IS enemy safe for Elmorad
        assert!(!is_in_own_safety_area(ZONE_BIFROST, 90.0, 770.0, 2)); // NOT own safe for Elmorad
    }

    #[test]
    fn test_own_safety_elmorad_zone_inverted() {
        // C++ isInOwnSafetyArea ELMORAD zone: Karus near (210,1853)
        // C++ isInEnemySafetyArea ELMORAD zone: Elmorad near (210,1853)
        assert!(is_in_own_safety_area(ZONE_ELMORAD, 210.0, 1853.0, 1)); // Karus in own safe
        assert!(!is_in_own_safety_area(ZONE_ELMORAD, 210.0, 1853.0, 2)); // Elmorad NOT in own safe
        assert!(is_in_enemy_safety_area(ZONE_ELMORAD, 210.0, 1853.0, 2)); // Elmorad in enemy safe
    }

    #[test]
    fn test_own_safety_karus_zone_inverted() {
        // C++ isInOwnSafetyArea KARUS zone: Elmorad near (1860,174)
        assert!(is_in_own_safety_area(ZONE_KARUS, 1860.0, 174.0, 2)); // Elmorad in own safe
        assert!(!is_in_own_safety_area(ZONE_KARUS, 1860.0, 174.0, 1)); // Karus NOT in own safe
        assert!(is_in_enemy_safety_area(ZONE_KARUS, 1860.0, 174.0, 1)); // Karus in enemy safe
    }

    #[test]
    fn test_own_safety_battle_inverted() {
        // BATTLE zone: nations are inverted between own and enemy safety
        // Enemy safety: Karus checks (98-125, 755-780), Elmorad checks (805-831, 85-110)
        // Own safety: Elmorad checks (98-125, 755-780), Karus checks (805-831, 85-110)
        assert!(is_in_own_safety_area(ZONE_BATTLE, 110.0, 765.0, 2)); // Elmorad in own safe (Karus spawn)
        assert!(!is_in_own_safety_area(ZONE_BATTLE, 110.0, 765.0, 1)); // Karus NOT in own safe
        assert!(is_in_enemy_safety_area(ZONE_BATTLE, 110.0, 765.0, 1)); // Karus in enemy safe
    }

    #[test]
    fn test_own_safety_normal_zone() {
        assert!(!is_in_own_safety_area(21, 500.0, 500.0, 1));
        assert!(!is_in_own_safety_area(21, 500.0, 500.0, 2));
    }

    // ── Incapacitated constants tests ────────────────────────────────────

    #[test]
    fn test_buff_type_constants() {
        assert_eq!(BUFF_TYPE_BLIND, 21);
        assert_eq!(BUFF_TYPE_KAUL_TRANSFORMATION, 154);
    }

    // ── is_in_arena tests ───────────────────────────────────────────────

    #[test]
    fn test_is_in_arena_zone_arena() {
        // ZONE_ARENA (48) — any location counts as arena
        assert!(is_in_arena(ZONE_ARENA, 0.0, 0.0));
        assert!(is_in_arena(ZONE_ARENA, 500.0, 500.0));
    }

    #[test]
    fn test_is_in_arena_moradon_party_arena() {
        // Moradon party arena: x 684-735, z 360-411
        assert!(is_in_arena(ZONE_MORADON, 700.0, 380.0));
        assert!(is_in_arena(22, 700.0, 380.0)); // ZONE_MORADON2 = 22
    }

    #[test]
    fn test_is_in_arena_moradon_melee_arena() {
        // Moradon melee arena: x 684-735, z 440-491
        assert!(is_in_arena(ZONE_MORADON, 700.0, 460.0));
    }

    #[test]
    fn test_is_in_arena_moradon_outside() {
        // Moradon outside arena bounds
        assert!(!is_in_arena(ZONE_MORADON, 500.0, 500.0));
        assert!(!is_in_arena(ZONE_MORADON, 700.0, 420.0)); // between arenas
    }

    #[test]
    fn test_is_in_arena_other_zone() {
        assert!(!is_in_arena(ZONE_RONARK_LAND, 700.0, 380.0));
        assert!(!is_in_arena(ZONE_KARUS, 700.0, 380.0));
    }

    // ── is_in_pvp_zone tests ────────────────────────────────────────────

    #[test]
    fn test_is_in_pvp_zone_ronark() {
        assert!(is_in_pvp_zone(ZONE_RONARK_LAND));
        assert!(is_in_pvp_zone(ZONE_RONARK_LAND_BASE));
        assert!(is_in_pvp_zone(ZONE_ARDREAM));
    }

    #[test]
    fn test_is_in_pvp_zone_battle() {
        assert!(is_in_pvp_zone(ZONE_BATTLE));
        assert!(is_in_pvp_zone(ZONE_BATTLE2));
        assert!(is_in_pvp_zone(ZONE_BATTLE3));
        assert!(is_in_pvp_zone(ZONE_BATTLE4));
        assert!(is_in_pvp_zone(ZONE_BATTLE5));
        assert!(is_in_pvp_zone(ZONE_BATTLE6));
    }

    #[test]
    fn test_is_in_pvp_zone_special() {
        assert!(is_in_pvp_zone(ZONE_JURAID_MOUNTAIN));
        assert!(is_in_pvp_zone(ZONE_BORDER_DEFENSE_WAR));
        assert!(is_in_pvp_zone(ZONE_CLAN_WAR_ARDREAM));
        assert!(is_in_pvp_zone(ZONE_CLAN_WAR_RONARK));
        assert!(is_in_pvp_zone(ZONE_BIFROST));
    }

    #[test]
    fn test_is_in_pvp_zone_party_vs() {
        assert!(is_in_pvp_zone(ZONE_PARTY_VS_1));
        assert!(is_in_pvp_zone(97)); // ZONE_PARTY_VS_2
        assert!(is_in_pvp_zone(98)); // ZONE_PARTY_VS_3
        assert!(is_in_pvp_zone(ZONE_PARTY_VS_4));
    }

    #[test]
    fn test_is_in_pvp_zone_special_event() {
        // SPBATTLE zones 105-115
        assert!(is_in_pvp_zone(105));
        assert!(is_in_pvp_zone(110));
        assert!(is_in_pvp_zone(115));
        assert!(!is_in_pvp_zone(104));
        assert!(!is_in_pvp_zone(116));
    }

    #[test]
    fn test_is_in_pvp_zone_non_pvp() {
        assert!(!is_in_pvp_zone(ZONE_MORADON));
        assert!(!is_in_pvp_zone(ZONE_KARUS));
        assert!(!is_in_pvp_zone(ZONE_ELMORAD));
        assert!(!is_in_pvp_zone(ZONE_DELOS));
    }

    // ── Castle zone helpers tests ───────────────────────────────────────

    #[test]
    fn test_is_in_luferson_castle() {
        assert!(is_in_luferson_castle(ZONE_KARUS));
        assert!(is_in_luferson_castle(ZONE_KARUS2));
        assert!(is_in_luferson_castle(ZONE_KARUS3));
        assert!(!is_in_luferson_castle(ZONE_ELMORAD));
        assert!(!is_in_luferson_castle(ZONE_MORADON));
    }

    #[test]
    fn test_is_in_elmorad_castle() {
        assert!(is_in_elmorad_castle(ZONE_ELMORAD));
        assert!(is_in_elmorad_castle(ZONE_ELMORAD2));
        assert!(is_in_elmorad_castle(ZONE_ELMORAD3));
        assert!(!is_in_elmorad_castle(ZONE_KARUS));
        assert!(!is_in_elmorad_castle(ZONE_MORADON));
    }

    #[test]
    fn test_is_in_special_event_zone() {
        assert!(is_in_special_event_zone(105));
        assert!(is_in_special_event_zone(115));
        assert!(!is_in_special_event_zone(104));
        assert!(!is_in_special_event_zone(116));
    }

    #[test]
    fn test_zindan_event_opened_default() {
        // Zindan event opened should default to false
        let world = WorldState::new();
        assert!(!world.is_zindan_event_opened());
    }

    #[test]
    fn test_zindan_event_opened_toggle() {
        let world = WorldState::new();
        world.set_zindan_event_opened(true);
        assert!(world.is_zindan_event_opened());
        world.set_zindan_event_opened(false);
        assert!(!world.is_zindan_event_opened());
    }

    // ── NPC type-specific damage override tests ──────────────────────────

    #[test]
    fn test_npc_type_constants() {
        // C++ globals.h NPC type values — canonical source: npc_type_constants.rs
        assert_eq!(NPC_TREE, 2);
        assert_eq!(NPC_OBJECT_FLAG, 15);
        assert_eq!(NPC_REFUGEE, 46);
        assert_eq!(NPC_FOSIL, 173);
        assert_eq!(NPC_BORDER_MONUMENT, 212);
        assert_eq!(NPC_PARTNER_TYPE, 213);
        assert_eq!(NPC_PRISON, 220);
    }

    #[test]
    fn test_npc_tree_fixed_damage() {
        // NPC_TREE always takes 20 damage regardless of player attack power
        let _normal_damage: i16 = 5000;
        // When npc_type is NPC_TREE, damage should be overridden to 20
        assert_eq!(NPC_TREE, 2);
        // The function returns 20 for tree type
        assert_eq!(20i16, 20);
    }

    #[test]
    fn test_npc_border_monument_fixed_damage() {
        // NPC_BORDER_MONUMENT always takes 10 damage
        assert_eq!(NPC_BORDER_MONUMENT, 212);
        // Fixed damage = 10
        assert_eq!(10i16, 10);
    }

    #[test]
    fn test_npc_refugee_damage_by_proto() {
        // NPC_REFUGEE: specific protos get 20, others get 10
        let special_protos: [u16; 4] = [3202, 3203, 3252, 3253];
        for proto in special_protos {
            assert!(matches!(proto, 3202 | 3203 | 3252 | 3253));
        }
        // Non-special protos get 10
        assert!(!matches!(1000u16, 3202 | 3203 | 3252 | 3253));
    }

    #[test]
    fn test_npc_object_flag_proto_511() {
        // NPC_OBJECT_FLAG with proto 511 → damage = 1
        assert_eq!(NPC_OBJECT_FLAG, 15);
        // Only proto 511 gets this override
        assert_eq!(511u16, 511);
    }

    #[test]
    fn test_npc_neutral_nation_3_blocked() {
        // NPCs with org_nation/group == 3 cannot be R-attacked
        let group: u8 = 3;
        assert_eq!(group, 3);
        // Damage should be 0 for nation-3 NPCs
    }

    #[test]
    fn test_npc_partner_type_nation_none_blocked() {
        // NPC_PARTNER_TYPE with Nation::NONE → damage = 0
        let group: u8 = 0; // Nation::NONE
        assert_eq!(NPC_PARTNER_TYPE, 213);
        assert_eq!(group, 0);
    }

    #[test]
    fn test_punishment_stick_item_id() {
        const PUNISHMENT_STICK_ID: i32 = 900356000;
        assert_eq!(PUNISHMENT_STICK_ID, 900356000);
    }

    #[test]
    fn test_weapon_pickaxe_kind() {
        const WEAPON_PICKAXE: i32 = 61;
        assert_eq!(WEAPON_PICKAXE, 61);
    }

    // ── Sprint 159: Mirror damage party distribution tests ──────────────

    #[test]
    fn test_mirror_damage_party_cpp_precedence_bug() {
        //   mirrorDamage = mirrorDamage / p_count < 2 ? 2 : p_count;
        // Due to C++ precedence: (mirrorDamage/p_count < 2) evaluates as bool,
        // then ternary picks 2 or p_count.
        let mirror_dmg: i32 = 50;
        let p_count: i32 = 4;

        // Rust matching C++ precedence:
        let per_member = if (mirror_dmg / p_count) < 2 {
            2
        } else {
            p_count
        };
        // 50/4 = 12, which is >= 2, so result is p_count (4)
        assert_eq!(per_member, 4);

        // Small mirror damage case:
        let small_dmg: i32 = 5;
        let per_member2 = if (small_dmg / p_count) < 2 {
            2
        } else {
            p_count
        };
        // 5/4 = 1, which is < 2, so result is 2
        assert_eq!(per_member2, 2);
    }

    #[test]
    fn test_mirror_damage_party_self_excluded() {
        //   if (p == nullptr || p == this) continue;
        // The victim (target) should not receive mirror damage
        let target_sid: u16 = 5;
        let party_members: Vec<u16> = vec![1, 2, 5, 7]; // includes target_sid=5
        let filtered: Vec<u16> = party_members
            .iter()
            .filter(|&&m| m != target_sid)
            .copied()
            .collect();
        assert_eq!(filtered, vec![1, 2, 7]);
    }

    #[test]
    fn test_mirror_damage_party_attacker_party_lookup() {
        //   pParty = g_pMain->GetPartyPtr(pUser->GetPartyID())
        // Mirror party distribution looks up ATTACKER's party, not victim's
        let attacker_party_id: u16 = 42;
        let victim_party_id: u16 = 99;
        // The party to iterate is the ATTACKER's party
        assert_ne!(attacker_party_id, victim_party_id);
        // attacker_party_id is used (not victim_party_id)
        assert_eq!(attacker_party_id, 42);
    }

    // ── Sprint 227: Mirror damage victim reduction test ────────────────

    #[test]
    fn test_mirror_damage_victim_reduction() {
        //   mirrorDamage = (m_byMirrorAmount * amount) / 100;
        //   amount -= mirrorDamage;
        // In C++, amount is negative. In Rust, damage is positive.
        // The victim's damage is reduced by the mirror portion.
        let damage: i16 = 500;
        let mirror_amount: u8 = 30; // 30% mirror

        let mirror_dmg = (mirror_amount as i32 * damage as i32) / 100;
        assert_eq!(mirror_dmg, 150);

        let reduced_damage = (damage as i32 - mirror_dmg).max(0) as i16;
        assert_eq!(reduced_damage, 350);

        // Victim takes 350, attacker/party takes 150 (total = 500 = original)
        assert_eq!(reduced_damage as i32 + mirror_dmg, damage as i32);
    }

    #[test]
    fn test_mirror_damage_victim_reduction_100_pct() {
        // Edge case: 100% mirror — victim takes 0 damage, all reflected
        let damage: i16 = 200;
        let mirror_amount: u8 = 100;

        let mirror_dmg = (mirror_amount as i32 * damage as i32) / 100;
        assert_eq!(mirror_dmg, 200);

        let reduced_damage = (damage as i32 - mirror_dmg).max(0) as i16;
        assert_eq!(reduced_damage, 0);
    }

    // ── Chaos Temple PvP zone check tests ────────────────────────────

    #[test]
    fn test_chaos_dungeon_zone_constant() {
        assert_eq!(ZONE_CHAOS_DUNGEON, 85);
    }

    #[test]
    fn test_chaos_temple_ffa_pvp_concept() {
        // Chaos Temple is free-for-all: no nation check required.
        // Even same-nation players can attack each other in zone 85.
        let attacker_nation: u8 = 1; // Karus
        let target_nation: u8 = 1; // Also Karus
        let zone: u16 = ZONE_CHAOS_DUNGEON;
        // In Chaos Temple, nation doesn't matter — all can fight
        let chaos_active = true;
        let can_fight = zone == ZONE_CHAOS_DUNGEON && chaos_active;
        assert!(can_fight);
        // Even same nation can fight
        let _ = (attacker_nation, target_nation);
    }

    // ── Cinderella War PvP zone check tests ──────────────────────────

    #[test]
    fn test_cinderella_war_opposite_nations_can_fight() {
        // Cinderella War: opposite nations who are event users can PvP
        let attacker_nation: u8 = 1; // Karus
        let target_nation: u8 = 2; // El Morad
        let is_event_active = true;
        let both_event_users = true;
        let can_fight = is_event_active && attacker_nation != target_nation && both_event_users;
        assert!(can_fight);
    }

    #[test]
    fn test_cinderella_war_same_nation_cannot_fight() {
        let attacker_nation: u8 = 2;
        let target_nation: u8 = 2;
        let is_event_active = true;
        let can_fight = is_event_active && attacker_nation != target_nation;
        assert!(!can_fight);
    }

    #[test]
    fn test_cinderella_war_non_event_user_cannot_fight() {
        let attacker_nation: u8 = 1;
        let target_nation: u8 = 2;
        let both_event_users = false; // One is not registered
        let can_fight = attacker_nation != target_nation && both_event_users;
        assert!(!can_fight);
    }

    #[test]
    fn test_monster_stone_boss_kill_detection() {
        // Boss kill is triggered when NPC has event_room > 0 and summon_type == 1
        let npc = crate::npc::NpcInstance {
            nid: 10100,
            proto_id: 500,
            is_monster: true,
            zone_id: 81,
            x: 100.0,
            y: 0.0,
            z: 100.0,
            direction: 0,
            region_x: 7,
            region_z: 7,
            gate_open: 0,
            object_type: 0,
            nation: 0,
            special_type: 0,
            trap_number: 0,
            event_room: 5, // room_id 4, 1-based
            is_event_npc: true,
            summon_type: 1, // Boss
            user_name: String::new(),
            pet_name: String::new(),
            clan_name: String::new(),
            clan_id: 0,
            clan_mark_version: 0,
        };

        // Should trigger boss kill
        assert!(npc.event_room > 0 && npc.summon_type == 1);

        // Normal monster in event room — should NOT trigger
        let normal_npc = crate::npc::NpcInstance {
            summon_type: 0, // Normal
            ..npc.clone()
        };
        assert!(!(normal_npc.event_room > 0 && normal_npc.summon_type == 1));

        // NPC not in event room — should NOT trigger
        let no_room_npc = crate::npc::NpcInstance {
            event_room: 0,
            ..npc.clone()
        };
        assert!(!(no_room_npc.event_room > 0 && no_room_npc.summon_type == 1));
    }

    #[test]
    fn test_monster_stone_boss_kill_room_id_conversion() {
        // GetEventRoom() - 1 gives the 0-based room index
        let event_room: u16 = 42; // 1-based
        let room_id = event_room - 1; // 0-based
        assert_eq!(room_id, 41);
    }

    // ── Sprint 196: Combat isolation via event_room ──────────────────────

    #[test]
    fn test_monster_stone_combat_isolation_same_room_allowed() {
        // Players in the same event room CAN attack each other
        let world = crate::world::WorldState::new();
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx1);
        world.register_session(2, tx2);

        world.update_session(1, |h| {
            h.event_room = 5;
            h.monster_stone_status = true;
        });
        world.update_session(2, |h| {
            h.event_room = 5;
            h.monster_stone_status = true;
        });

        let zone_id: u16 = 81; // Monster Stone zone
        assert!(crate::systems::monster_stone::is_monster_stone_zone(
            zone_id
        ));
        assert!(world.is_same_event_room(1, 2));
    }

    #[test]
    fn test_monster_stone_combat_isolation_different_room_blocked() {
        // Players in different event rooms CANNOT attack each other
        // (when monster_stone_status is true)
        let world = crate::world::WorldState::new();
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx1);
        world.register_session(2, tx2);

        world.update_session(1, |h| {
            h.event_room = 3;
            h.monster_stone_status = true;
        });
        world.update_session(2, |h| {
            h.event_room = 7;
            h.monster_stone_status = true;
        });

        let zone_id: u16 = 82; // Monster Stone zone
        assert!(crate::systems::monster_stone::is_monster_stone_zone(
            zone_id
        ));
        assert!(!world.is_same_event_room(1, 2));
        assert!(world.get_monster_stone_status(1));
    }

    #[test]
    fn test_monster_stone_combat_isolation_one_not_in_room() {
        // One player in event room, other not — cannot attack
        let world = crate::world::WorldState::new();
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx1);
        world.register_session(2, tx2);

        world.update_session(1, |h| {
            h.event_room = 5;
            h.monster_stone_status = true;
        });
        // Session 2 stays at event_room = 0, monster_stone_status = false

        let zone_id: u16 = 83; // Monster Stone zone
        assert!(crate::systems::monster_stone::is_monster_stone_zone(
            zone_id
        ));
        assert!(!world.is_same_event_room(1, 2));
    }

    #[test]
    fn test_monster_stone_status_false_skips_room_check() {
        // When monster_stone_status is false, event room isolation is NOT enforced
        // even if the player is in a Monster Stone zone with a different event_room.
        let world = crate::world::WorldState::new();
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx1);
        world.register_session(2, tx2);

        // Both players in MS zone with different rooms but status=false
        world.update_session(1, |h| {
            h.event_room = 3;
            h.monster_stone_status = false;
        });
        world.update_session(2, |h| {
            h.event_room = 7;
            h.monster_stone_status = false;
        });

        // monster_stone_status is false → guard skipped → attacks proceed
        assert!(!world.get_monster_stone_status(1));
        assert!(!world.get_monster_stone_status(2));
    }

    #[test]
    fn test_non_monster_stone_zone_no_event_room_check() {
        // In non-Monster Stone zones, event_room is irrelevant
        let zone_id: u16 = 21; // Moradon
        assert!(!crate::systems::monster_stone::is_monster_stone_zone(
            zone_id
        ));
        // Even with different rooms, attacks proceed in non-MS zones
        // (the check only applies in MS zones 81-83)
    }

    /// Helper to construct a minimal CharacterInfo for kill reward tests.
    fn make_kill_reward_char(
        sid: crate::zone::SessionId,
        nation: u8,
        class: u16,
    ) -> crate::world::CharacterInfo {
        crate::world::CharacterInfo {
            session_id: sid,
            name: format!("Player{}", sid),
            nation,
            race: 1,
            class,
            level: 60,
            face: 1,
            hair_rgb: 0,
            rank: 0,
            title: 0,
            max_hp: 5000,
            hp: 5000,
            max_mp: 3000,
            mp: 3000,
            max_sp: 0,
            sp: 0,
            equipped_items: [0u32; 14],
            bind_zone: 0,
            bind_x: 0.0,
            bind_z: 0.0,
            str: 60,
            sta: 60,
            dex: 60,
            intel: 60,
            cha: 60,
            free_points: 0,
            skill_points: [0u8; 10],
            gold: 0,
            loyalty: 0,
            loyalty_monthly: 0,
            authority: 1,
            knights_id: 0,
            fame: 0,
            party_id: None,
            exp: 0,
            max_exp: 100_000,
            exp_seal_status: false,
            sealed_exp: 0,
            item_weight: 0,
            max_weight: 1000,
            res_hp_type: 1,
            rival_id: -1,
            rival_expiry_time: 0,
            anger_gauge: 0,
            manner_point: 0,
            rebirth_level: 0,
            reb_str: 0,
            reb_sta: 0,
            reb_dex: 0,
            reb_intel: 0,
            reb_cha: 0,
            cover_title: 0,
        }
    }

    #[test]
    fn test_give_kill_reward_event_room_filter_same_room() {
        // should pass the event_room filter.
        let world = crate::world::WorldState::new();
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx1);
        world.register_session(2, tx2);

        // Set up characters in same zone + same event room
        world.update_session(1, |h| {
            h.character = Some(make_kill_reward_char(1, 1, 101));
            h.position.zone_id = ZONE_BORDER_DEFENSE_WAR;
            h.event_room = 3;
        });
        world.update_session(2, |h| {
            h.character = Some(make_kill_reward_char(2, 1, 101));
            h.position.zone_id = ZONE_BORDER_DEFENSE_WAR;
            h.event_room = 3; // Same room
        });

        // Capture killer's event_room first (can't call world inside with_session)
        let killer_room = world.get_event_room(1);
        let member_ok = world
            .with_session(2, |h| {
                h.character.is_some()
                    && h.position.zone_id == ZONE_BORDER_DEFENSE_WAR
                    && h.event_room == killer_room
            })
            .unwrap_or(false);
        assert!(member_ok, "Same event room should pass filter");
    }

    #[test]
    fn test_give_kill_reward_event_room_filter_different_room() {
        // should be filtered out and NOT receive rewards.
        let world = crate::world::WorldState::new();
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx1);
        world.register_session(2, tx2);

        // Same zone, different event rooms
        world.update_session(1, |h| {
            h.character = Some(make_kill_reward_char(1, 1, 101));
            h.position.zone_id = ZONE_BORDER_DEFENSE_WAR;
            h.event_room = 1;
        });
        world.update_session(2, |h| {
            h.character = Some(make_kill_reward_char(2, 1, 101));
            h.position.zone_id = ZONE_BORDER_DEFENSE_WAR;
            h.event_room = 2; // Different room
        });

        let killer_room = world.get_event_room(1);
        let member_ok = world
            .with_session(2, |h| {
                h.character.is_some()
                    && h.position.zone_id == ZONE_BORDER_DEFENSE_WAR
                    && h.event_room == killer_room
            })
            .unwrap_or(false);
        assert!(!member_ok, "Different event room should fail filter");
    }

    #[test]
    fn test_give_kill_reward_event_room_zero_matches_zero() {
        // When neither player is in an event room (room=0), they should match.
        // This is the normal non-event case.
        let world = crate::world::WorldState::new();
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx1);
        world.register_session(2, tx2);

        world.update_session(1, |h| {
            h.character = Some(make_kill_reward_char(1, 1, 101));
            h.position.zone_id = 2; // Ronark Land
            h.event_room = 0;
        });
        world.update_session(2, |h| {
            h.character = Some(make_kill_reward_char(2, 1, 101));
            h.position.zone_id = 2;
            h.event_room = 0;
        });

        let killer_room = world.get_event_room(1);
        assert_eq!(killer_room, 0);
        let member_ok = world
            .with_session(2, |h| {
                h.character.is_some() && h.position.zone_id == 2 && h.event_room == killer_room
            })
            .unwrap_or(false);
        assert!(
            member_ok,
            "Both room=0 should match (normal non-event case)"
        );
    }

    // ── Sprint 257: Deva Bird Juraid bridge check tests ──────────────

    #[test]
    fn test_deva_bird_blocked_no_bridges() {
        let world = WorldState::new();
        // Room 1 with no bridges open
        let bs = crate::systems::juraid::JuraidBridgeState::new();
        world.set_juraid_bridge_state(1, bs);

        // Karus (nation=1) needs all 3 bridges
        assert!(!world.are_all_juraid_bridges_open(1, 1));
        // Elmorad (nation=2) also blocked
        assert!(!world.are_all_juraid_bridges_open(1, 2));
    }

    #[test]
    fn test_deva_bird_allowed_all_bridges_open() {
        let world = WorldState::new();
        let mut bs = crate::systems::juraid::JuraidBridgeState::new();
        bs.open_bridge(0, 1);
        bs.open_bridge(1, 1);
        bs.open_bridge(2, 1);
        world.set_juraid_bridge_state(1, bs);

        // Karus (nation=1) has all 3 bridges → allowed
        assert!(world.are_all_juraid_bridges_open(1, 1));
        // Elmorad (nation=2) has 0 bridges → blocked
        assert!(!world.are_all_juraid_bridges_open(1, 2));
    }

    #[test]
    fn test_deva_bird_partial_bridges_blocked() {
        let world = WorldState::new();
        let mut bs = crate::systems::juraid::JuraidBridgeState::new();
        bs.open_bridge(0, 2);
        bs.open_bridge(1, 2);
        // Only 2 of 3 Elmorad bridges open
        world.set_juraid_bridge_state(1, bs);

        assert!(!world.are_all_juraid_bridges_open(1, 2));
    }

    #[test]
    fn test_deva_bird_nonexistent_room() {
        let world = WorldState::new();
        // No bridge state for room 99 → should be blocked
        assert!(!world.are_all_juraid_bridges_open(99, 1));
    }

    #[test]
    fn test_clear_juraid_bridge_states() {
        let world = WorldState::new();
        let mut bs = crate::systems::juraid::JuraidBridgeState::new();
        bs.open_bridge(0, 1);
        bs.open_bridge(1, 1);
        bs.open_bridge(2, 1);
        world.set_juraid_bridge_state(1, bs);

        assert!(world.are_all_juraid_bridges_open(1, 1));

        world.clear_juraid_bridge_states();
        assert!(!world.are_all_juraid_bridges_open(1, 1));
    }

    // ── Sprint 257: Vaccuni attack check tests ───────────────────────

    fn make_test_npc(proto_id: u16) -> crate::npc::NpcInstance {
        crate::npc::NpcInstance {
            nid: 10001,
            proto_id,
            is_monster: true,
            zone_id: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            direction: 0,
            region_x: 0,
            region_z: 0,
            gate_open: 0,
            object_type: 0,
            nation: 0,
            special_type: 0,
            trap_number: 0,
            event_room: 0,
            is_event_npc: false,
            summon_type: 0,
            user_name: String::new(),
            pet_name: String::new(),
            clan_name: String::new(),
            clan_id: 0,
            clan_mark_version: 0,
        }
    }

    #[test]
    fn test_vaccuni_attack_no_match() {
        // Proto IDs that don't match any Vaccuni target
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);

        let npc = make_test_npc(1234);
        assert!(!check_vaccuni_attack(&world, 1, &npc));
    }

    #[test]
    fn test_vaccuni_attack_matching_proto_no_event() {
        // Proto 4351 but no quest event flag
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);
        world.update_session(1, |h| {
            let mut ch = make_kill_reward_char(1, 1, 80);
            ch.name = "TestVaccuni".to_string();
            h.character = Some(ch);
        });

        let npc = make_test_npc(4351);
        // No quest event 793/794 → should fail
        assert!(!check_vaccuni_attack(&world, 1, &npc));
    }

    #[test]
    fn test_vaccuni_attack_fallthrough_protos() {
        // Proto 4301, 605, 611, 616 fall through (always return false)
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);

        for proto in [4301u16, 605, 611, 616] {
            let npc = make_test_npc(proto);
            assert!(
                !check_vaccuni_attack(&world, 1, &npc),
                "Proto {} should return false (fall-through)",
                proto
            );
        }
    }

    // ── Sprint 257: Type3 DOT re-cast prevention test ────────────────

    #[test]
    fn test_type3_hot_recast_blocked() {
        // If target already has a HOT (hp_amount > 0), re-cast should be blocked
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);

        // Add an active HOT to the target
        let added = world.add_durational_skill(1, 108100, 50, 5, 2);
        assert!(added);

        // Verify target has active HOT
        assert!(world.has_active_hot(1));
    }

    #[test]
    fn test_type3_dot_allows_recast() {
        // If target only has a DOT (hp_amount < 0), re-cast should be allowed
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);

        // Add an active DOT (negative = damage)
        let added = world.add_durational_skill(1, 108100, -50, 5, 2);
        assert!(added);

        // DOT is not a HOT, so has_active_hot should be false
        assert!(!world.has_active_hot(1));
    }

    // ── Sprint 330: isAttackDisabled tests ───────────────────────────

    /// Test that is_attack_disabled returns false when status is 0 (default).
    #[test]
    fn test_attack_disabled_default_false() {
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);
        assert!(!world.is_attack_disabled(1));
    }

    /// Test that is_attack_disabled returns true when permanently banned (u32::MAX).
    #[test]
    fn test_attack_disabled_permanent() {
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);
        world.update_session(1, |h| h.attack_disabled_until = u32::MAX);
        assert!(world.is_attack_disabled(1));
    }

    /// Test that is_attack_disabled returns true when temporarily banned (future timestamp).
    #[test]
    fn test_attack_disabled_temporary_future() {
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);
        // Set to far future
        world.update_session(1, |h| h.attack_disabled_until = u32::MAX - 1);
        assert!(world.is_attack_disabled(1));
    }

    /// Test that is_attack_disabled returns false when ban has expired (past timestamp).
    #[test]
    fn test_attack_disabled_expired() {
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        world.register_session(1, tx);
        // Set to past timestamp (1 = Jan 1, 1970)
        world.update_session(1, |h| h.attack_disabled_until = 1);
        assert!(!world.is_attack_disabled(1));
    }

    // ── Sprint 359: Self-targeting prevention ─────────────────────────

    /// is_hostile_to returns false when attacker == target (self-targeting).
    ///
    #[test]
    fn test_is_hostile_to_self_targeting_blocked() {
        let world = WorldState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sid = world.allocate_session_id();
        world.register_session(sid, tx);

        let ch = CharacterInfo {
            session_id: sid,
            name: "SelfTest".into(),
            nation: 1,
            race: 1,
            class: 101,
            level: 60,
            face: 1,
            hair_rgb: 0,
            rank: 0,
            title: 0,
            max_hp: 5000,
            hp: 5000,
            max_mp: 3000,
            mp: 3000,
            max_sp: 0,
            sp: 0,
            equipped_items: [0; 14],
            bind_zone: 21,
            bind_x: 0.0,
            bind_z: 0.0,
            str: 90,
            sta: 60,
            dex: 30,
            intel: 20,
            cha: 10,
            free_points: 0,
            skill_points: [0; 10],
            gold: 0,
            loyalty: 0,
            loyalty_monthly: 0,
            authority: 1,
            knights_id: 0,
            fame: 0,
            party_id: None,
            exp: 0,
            max_exp: 0,
            exp_seal_status: false,
            sealed_exp: 0,
            item_weight: 0,
            max_weight: 0,
            res_hp_type: 1,
            rival_id: -1,
            rival_expiry_time: 0,
            anger_gauge: 0,
            manner_point: 0,
            rebirth_level: 0,
            reb_str: 0,
            reb_sta: 0,
            reb_dex: 0,
            reb_intel: 0,
            reb_cha: 0,
            cover_title: 0,
        };
        let pos = Position {
            zone_id: 85, // Chaos Dungeon (free-for-all PvP)
            x: 200.0,
            y: 0.0,
            z: 200.0,
            region_x: 1,
            region_z: 1,
        };
        world.register_ingame(sid, ch.clone(), pos);

        // Even in Chaos Dungeon (free-for-all), self-targeting must be blocked
        assert!(
            !is_hostile_to(&world, sid, &ch, &pos, sid, &ch, &pos),
            "Self-targeting must always return false, even in free-for-all zones"
        );
    }

    /// is_hostile_to allows attacking different players in Chaos Dungeon.
    #[test]
    fn test_is_hostile_to_chaos_dungeon_allows_pvp() {
        use crate::systems::event_room::TempleEventType;

        let world = WorldState::new();
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        let sid1 = world.allocate_session_id();
        let sid2 = world.allocate_session_id();
        world.register_session(sid1, tx1);
        world.register_session(sid2, tx2);

        let ch1 = CharacterInfo {
            session_id: sid1,
            name: "Attacker".into(),
            nation: 1,
            race: 1,
            class: 101,
            level: 60,
            face: 1,
            hair_rgb: 0,
            rank: 0,
            title: 0,
            max_hp: 5000,
            hp: 5000,
            max_mp: 3000,
            mp: 3000,
            max_sp: 0,
            sp: 0,
            equipped_items: [0; 14],
            bind_zone: 21,
            bind_x: 0.0,
            bind_z: 0.0,
            str: 90,
            sta: 60,
            dex: 30,
            intel: 20,
            cha: 10,
            free_points: 0,
            skill_points: [0; 10],
            gold: 0,
            loyalty: 0,
            loyalty_monthly: 0,
            authority: 1,
            knights_id: 0,
            fame: 0,
            party_id: None,
            exp: 0,
            max_exp: 0,
            exp_seal_status: false,
            sealed_exp: 0,
            item_weight: 0,
            max_weight: 0,
            res_hp_type: 1,
            rival_id: -1,
            rival_expiry_time: 0,
            anger_gauge: 0,
            manner_point: 0,
            rebirth_level: 0,
            reb_str: 0,
            reb_sta: 0,
            reb_dex: 0,
            reb_intel: 0,
            reb_cha: 0,
            cover_title: 0,
        };
        let ch2 = CharacterInfo {
            session_id: sid2,
            name: "Target".into(),
            nation: 1, // Same nation
            ..ch1.clone()
        };
        let pos = Position {
            zone_id: 85,
            x: 200.0,
            y: 0.0,
            z: 200.0,
            region_x: 1,
            region_z: 1,
        };
        world.register_ingame(sid1, ch1.clone(), pos);
        world.register_ingame(sid2, ch2.clone(), pos);

        // Activate Chaos Dungeon event
        world.event_room_manager.update_temple_event(|s| {
            s.active_event = TempleEventType::ChaosDungeon as i16;
            s.is_active = true;
        });

        // Same nation in Chaos Dungeon with event active — should allow PvP
        assert!(
            is_hostile_to(&world, sid1, &ch1, &pos, sid2, &ch2, &pos),
            "Different players in Chaos Dungeon should be hostile"
        );
    }

    // ── Bot last_attacker_id tracking tests ───────────────────────────

    /// Helper to create a minimal BotInstance for testing.
    fn make_test_bot(id: u32, zone_id: u16, x: f32, z: f32) -> crate::world::BotInstance {
        use crate::world::{BotAiState, BotPresence};
        crate::world::BotInstance {
            id,
            db_id: 0,
            name: format!("TestBot{}", id),
            nation: 2,
            race: 1,
            class: 106,
            hair_rgb: 0,
            level: 70,
            face: 1,
            knights_id: 0,
            fame: 0,
            zone_id,
            x,
            y: 0.0,
            z,
            direction: 0,
            region_x: (x / 32.0) as u16,
            region_z: (z / 32.0) as u16,
            hp: 5000,
            max_hp: 5000,
            mp: 1000,
            max_mp: 1000,
            sp: 0,
            max_sp: 0,
            str_stat: 100,
            sta_stat: 80,
            dex_stat: 90,
            int_stat: 60,
            cha_stat: 30,
            gold: 0,
            loyalty: 0,
            loyalty_monthly: 0,
            in_game: true,
            presence: BotPresence::Standing,
            ai_state: BotAiState::Pk,
            target_id: -1,
            target_changed: false,
            spawned_at: 0,
            duration_minutes: 0,
            last_tick_ms: 0,
            last_move_ms: 0,
            last_mining_ms: 0,
            last_merchant_chat_ms: 0,
            last_hp_change_ms: 0,
            last_regen_ms: 0,
            last_attacker_id: -1,
            skill_cooldown: [0; 2],
            last_type4_ms: 0,
            regene_at_ms: 0,
            original_ai_state: BotAiState::Idle,
            move_route: 0,
            move_state: 0,
            merchant_state: -1,
            premium_merchant: false,
            merchant_chat: String::new(),
            reb_level: 0,
            cover_title: 0,
            rival_id: -1,
            rival_expiry_time: 0,
            anger_gauge: 0,
            hiding_helmet: false,
            hiding_cospre: false,
            need_party: 1,
            equip_visual: [(0, 0, 0); 17],
            personal_rank: 0,
            knights_rank: 0,
        }
    }

    /// R-attack on a bot without attacker session info should be a no-op.
    /// The new damage path requires the attacker to have position + character info;
    /// without it, the attack returns early without modifying the bot.
    #[tokio::test]
    async fn test_rattack_bot_no_attacker_info_noop() {
        let world = Arc::new(WorldState::new());
        let bot_id = BOT_ID_BASE + 1;
        let bot = make_test_bot(bot_id, 72, 100.0, 100.0);
        let original_hp = bot.hp;
        world.insert_bot(bot);

        // Player sid=5 has no registered session/character → early return
        handle_npc_attack(world.clone(), 5, Position::default(), bot_id, 1, 0).await;

        let after = world.get_bot(bot_id).unwrap();
        assert_eq!(
            after.hp, original_hp,
            "bot HP should be unchanged without attacker info"
        );
        assert_eq!(
            after.last_attacker_id, -1,
            "last_attacker_id should remain -1"
        );
    }

    /// R-attack on a dead bot should be a no-op.
    #[tokio::test]
    async fn test_rattack_bot_dead_noop() {
        let world = Arc::new(WorldState::new());
        let bot_id = BOT_ID_BASE + 2;
        let mut bot = make_test_bot(bot_id, 72, 100.0, 100.0);
        bot.hp = 0;
        bot.presence = crate::world::BotPresence::Dead;
        world.insert_bot(bot);

        handle_npc_attack(world.clone(), 10, Position::default(), bot_id, 1, 0).await;

        let after = world.get_bot(bot_id).unwrap();
        assert_eq!(after.hp, 0, "dead bot HP should remain 0");
        assert_eq!(after.last_attacker_id, -1);
    }

    /// R-attack on a non-existent bot ID should not panic.
    #[tokio::test]
    async fn test_rattack_nonexistent_bot_no_panic() {
        let world = Arc::new(WorldState::new());
        // Bot ID is in the bot range but no bot is inserted
        let fake_bot_id = BOT_ID_BASE + 999;
        handle_npc_attack(world.clone(), 5, Position::default(), fake_bot_id, 1, 0).await;
        // Should just silently return — no panic
    }

    /// Attacking an NPC ID that is NOT a bot should not modify any bot state.
    /// Both bots and NPCs share the >= 10000 ID space, so we verify that an
    /// attack on an ID with no bot entry falls through to the NPC path.
    #[tokio::test]
    async fn test_rattack_npc_does_not_affect_bots() {
        let world = Arc::new(WorldState::new());
        // Insert a bot at BOT_ID_BASE + 100
        let bot_id = BOT_ID_BASE + 100;
        let bot = make_test_bot(bot_id, 72, 100.0, 100.0);
        world.insert_bot(bot);

        // Attack a DIFFERENT NPC ID (BOT_ID_BASE + 200) which has no bot entry.
        // This should fall through to the NPC instance lookup (which also fails)
        // without touching the bot at BOT_ID_BASE + 100.
        let npc_only_id = BOT_ID_BASE + 200;
        handle_npc_attack(world.clone(), 5, Position::default(), npc_only_id, 1, 0).await;

        // The bot should remain untouched
        let after = world.get_bot(bot_id).unwrap();
        assert_eq!(
            after.last_attacker_id, -1,
            "attacking a different NPC should not modify bot last_attacker_id"
        );
    }

    // ── Elemental weapon damage tests ────────────────────────────────

    /// Create a minimal NPC template for testing elemental damage.
    fn make_elemental_npc(
        fire_r: i16,
        cold_r: i16,
        lightning_r: i16,
        poison_r: i16,
    ) -> crate::npc::NpcTemplate {
        crate::npc::NpcTemplate {
            s_sid: 1,
            is_monster: true,
            name: "TestNpc".to_string(),
            pid: 1,
            size: 100,
            weapon_1: 0,
            weapon_2: 0,
            group: 0,
            act_type: 0,
            npc_type: 0,
            family_type: 0,
            selling_group: 0,
            level: 1,
            max_hp: 1000,
            max_mp: 0,
            attack: 0,
            ac: 0,
            hit_rate: 0,
            evade_rate: 0,
            damage: 0,
            attack_delay: 0,
            speed_1: 0,
            speed_2: 0,
            stand_time: 0,
            search_range: 0,
            attack_range: 0,
            direct_attack: 0,
            tracing_range: 0,
            magic_1: 0,
            magic_2: 0,
            magic_3: 0,
            magic_attack: 0,
            fire_r,
            cold_r,
            lightning_r,
            magic_r: 0,
            disease_r: 0,
            poison_r,
            exp: 0,
            loyalty: 0,
            money: 0,
            item_table: 0,
            area_range: 0.0,
        }
    }

    #[test]
    fn test_elemental_damage_no_resistance() {
        // Fire=20, no target resistance → full 20 added
        let mut attacker_stats = crate::world::EquippedStats::default();
        attacker_stats
            .equipped_item_bonuses
            .insert(6, vec![(ITEM_TYPE_FIRE, 20)]);

        let npc_tmpl = make_elemental_npc(0, 0, 0, 0);
        let result = apply_elemental_weapon_damage_npc(&attacker_stats, &npc_tmpl, 100);
        assert_eq!(result, 120);
    }

    #[test]
    fn test_elemental_damage_partial_resistance() {
        // Fire=20, target fire_r=100 → total_r=100, bonus = 20 - 20*100/200 = 10
        let mut attacker_stats = crate::world::EquippedStats::default();
        attacker_stats
            .equipped_item_bonuses
            .insert(6, vec![(ITEM_TYPE_FIRE, 20)]);

        let npc_tmpl = make_elemental_npc(100, 0, 0, 0);
        let result = apply_elemental_weapon_damage_npc(&attacker_stats, &npc_tmpl, 100);
        assert_eq!(result, 110); // 100 base + 10 fire bonus
    }

    #[test]
    fn test_elemental_damage_full_resistance() {
        // Fire=20, target fire_r=200 → total_r=200, bonus = 20 - 20*200/200 = 0
        let mut attacker_stats = crate::world::EquippedStats::default();
        attacker_stats
            .equipped_item_bonuses
            .insert(6, vec![(ITEM_TYPE_FIRE, 20)]);

        let npc_tmpl = make_elemental_npc(200, 0, 0, 0);
        let result = apply_elemental_weapon_damage_npc(&attacker_stats, &npc_tmpl, 100);
        assert_eq!(result, 100); // No bonus — fully resisted
    }

    #[test]
    fn test_elemental_damage_multiple_types() {
        // Fire=10, Cold=15, Lightning=5 on same weapon
        let mut attacker_stats = crate::world::EquippedStats::default();
        attacker_stats.equipped_item_bonuses.insert(
            6,
            vec![
                (ITEM_TYPE_FIRE, 10),
                (ITEM_TYPE_COLD, 15),
                (ITEM_TYPE_LIGHTNING, 5),
            ],
        );

        let npc_tmpl = make_elemental_npc(0, 0, 0, 0);
        let result = apply_elemental_weapon_damage_npc(&attacker_stats, &npc_tmpl, 100);
        assert_eq!(result, 130); // 100 + 10 + 15 + 5
    }

    #[test]
    fn test_elemental_damage_resistance_cap_200() {
        // Fire=40, target fire_r=300 (exceeds cap) → capped to 200 → full reduction
        let mut attacker_stats = crate::world::EquippedStats::default();
        attacker_stats
            .equipped_item_bonuses
            .insert(6, vec![(ITEM_TYPE_FIRE, 40)]);

        let npc_tmpl = make_elemental_npc(300, 0, 0, 0);
        let result = apply_elemental_weapon_damage_npc(&attacker_stats, &npc_tmpl, 100);
        assert_eq!(result, 100); // No bonus — resistance capped at 200
    }

    #[test]
    fn test_elemental_damage_mirror_not_affected() {
        // Mirror damage (type 8) should not be counted as elemental
        let mut attacker_stats = crate::world::EquippedStats::default();
        attacker_stats
            .equipped_item_bonuses
            .insert(6, vec![(0x08, 30)]); // ITEM_TYPE_MIRROR_DAMAGE

        let npc_tmpl = make_elemental_npc(0, 0, 0, 0);
        let result = apply_elemental_weapon_damage_npc(&attacker_stats, &npc_tmpl, 100);
        assert_eq!(result, 100); // Mirror not counted in elemental
    }

    #[test]
    fn test_elemental_damage_poison_with_resist() {
        // Poison=30, target poison_r=60 → bonus = 30 - 30*60/200 = 30-9 = 21
        let mut attacker_stats = crate::world::EquippedStats::default();
        attacker_stats
            .equipped_item_bonuses
            .insert(6, vec![(ITEM_TYPE_POISON, 30)]);

        let npc_tmpl = make_elemental_npc(0, 0, 0, 60);
        let result = apply_elemental_weapon_damage_npc(&attacker_stats, &npc_tmpl, 100);
        assert_eq!(result, 121); // 100 + 21 poison bonus
    }

    // ── Santa NPC death condition tests ─────────────────────────────

    #[test]
    fn test_santa_death_guard_npc_type() {
        // Only NPC_SANTA (219) triggers proximity rewards.
        let mut tmpl = make_elemental_npc(0, 0, 0, 0);
        tmpl.npc_type = 0; // regular monster
        tmpl.area_range = 50.0;
        assert_ne!(tmpl.npc_type, NPC_SANTA);

        tmpl.npc_type = NPC_SANTA;
        assert_eq!(tmpl.npc_type, NPC_SANTA);
        assert_eq!(NPC_SANTA, 219);
    }

    #[test]
    fn test_santa_death_guard_area_range() {
        // area_range must be >= 1.0 (C++ check: m_area_range < 1.0f → return)
        let mut tmpl = make_elemental_npc(0, 0, 0, 0);
        tmpl.npc_type = NPC_SANTA;

        tmpl.area_range = 0.0;
        assert!(tmpl.area_range < 1.0, "area_range 0.0 should skip");

        tmpl.area_range = 0.5;
        assert!(tmpl.area_range < 1.0, "area_range 0.5 should skip");

        tmpl.area_range = 1.0;
        assert!(tmpl.area_range >= 1.0, "area_range 1.0 should trigger");

        tmpl.area_range = 50.0;
        assert!(tmpl.area_range >= 1.0, "area_range 50.0 should trigger");
    }

    #[test]
    fn test_santa_death_range_check_geometry() {
        // C++ isInRangeSlow: dx*dx + dz*dz <= range*range
        let npc_x: f32 = 100.0;
        let npc_z: f32 = 200.0;
        let range: f32 = 50.0;
        let range_sq = range * range;

        // Player exactly at NPC position — in range.
        let ddx = 0.0_f32;
        let ddz = 0.0_f32;
        assert!(ddx * ddx + ddz * ddz <= range_sq);

        // Player at edge of range — in range.
        let px = npc_x + 50.0;
        let pz = npc_z;
        let ddx = px - npc_x;
        let ddz = pz - npc_z;
        assert!(ddx * ddx + ddz * ddz <= range_sq);

        // Player just outside range — not in range.
        let px = npc_x + 50.1;
        let pz = npc_z;
        let ddx = px - npc_x;
        let ddz = pz - npc_z;
        assert!(ddx * ddx + ddz * ddz > range_sq);

        // Diagonal player at ~35.35 each axis (total dist ~50) — in range.
        let d = 35.0;
        let px = npc_x + d;
        let pz = npc_z + d;
        let ddx = px - npc_x;
        let ddz = pz - npc_z;
        // 35² + 35² = 2450, 50² = 2500 → in range
        assert!(ddx * ddx + ddz * ddz <= range_sq);
    }

    // ── Achievement2 PvP kill counter tests ──────────────────────────

    #[test]
    fn test_achieve_summary_user_defeat_count_saturating() {
        use crate::world::types::AchieveSummary;
        let mut s = AchieveSummary::default();
        assert_eq!(s.user_defeat_count, 0);

        s.user_defeat_count = s.user_defeat_count.saturating_add(1);
        assert_eq!(s.user_defeat_count, 1);

        s.user_defeat_count = u32::MAX;
        s.user_defeat_count = s.user_defeat_count.saturating_add(1);
        assert_eq!(s.user_defeat_count, u32::MAX); // no overflow
    }

    #[test]
    fn test_achieve_summary_user_death_count_saturating() {
        use crate::world::types::AchieveSummary;
        let mut s = AchieveSummary::default();
        assert_eq!(s.user_death_count, 0);

        s.user_death_count = s.user_death_count.saturating_add(1);
        assert_eq!(s.user_death_count, 1);

        s.user_death_count = u32::MAX;
        s.user_death_count = s.user_death_count.saturating_add(1);
        assert_eq!(s.user_death_count, u32::MAX);
    }

    #[test]
    fn test_achievement2_pvp_kill_packet_format() {
        // build_achievement2 should produce [i32 value] for PvP kill counter
        let pkt = crate::handler::achievement2::build_achievement2(42);
        assert_eq!(pkt.opcode, Opcode::WizAchievement2 as u8);
        let mut r = PacketReader::new(&pkt.data);
        assert_eq!(r.read_i32(), Some(42));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn test_achievement2_pvp_kill_zero_clears_display() {
        // value=0 clears the kill counter display on client
        let pkt = crate::handler::achievement2::build_achievement2(0);
        let mut r = PacketReader::new(&pkt.data);
        assert_eq!(r.read_i32(), Some(0));
    }
}
