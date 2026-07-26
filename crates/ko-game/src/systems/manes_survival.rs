//! Manes Survival runtime monster lifecycle.
//!
//! Client contract: Zones.tbl 570/580/590/600 map to server zones 57/58/59/60;
//! WIZ_SURVIVAL uses shared opcode byte 0xD0. Event monsters are runtime
//! NPCs and never belong in the global npc_spawn table.

use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::{DashMap, DashSet};
use ko_db::models::ManesSurvivalSpawnRow;
use parking_lot::RwLock;

use crate::world::WorldState;
use crate::zone::SessionId;

pub const ZONES_MANES_SURVIVAL: [u16; 4] = [57, 58, 59, 60];
/// Zones 57-60 are dedicated physical instances, so their NPCs share room 0
/// with players entering through the normal zone-change flow.
pub const MANES_EVENT_ROOM: u16 = 0;
pub const DARK_DRAGON_SID: i16 = 10733;
pub const MIN_ACTIVE_PARTICIPANTS: usize = 1;

const KARUS_ENTRY_START: (f32, f32) = (345.0, 207.0);
const KARUS_ENTRY_END: (f32, f32) = (260.0, 581.0);
const ELMORAD_ENTRY_START: (f32, f32) = (722.0, 644.0);
const ELMORAD_ENTRY_END: (f32, f32) = (728.0, 219.0);

pub const MANES_MAX_LEVEL: u8 = 30;

/// Medium-paced 20-minute progression. The 29 entries are the EXP required
/// to advance from levels 1..=29. Total EXP to level 30 is 11,020.
const MANES_LEVEL_EXP: [u16; 29] = [
    100, 120, 140, 160, 180, 200, 220, 240, 260, 280,
    300, 320, 340, 360, 380, 400, 420, 440, 460, 480,
    500, 520, 540, 560, 580, 600, 620, 640, 660,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManesProgress {
    pub level: u8,
    pub exp: u16,
    pub max_exp: u16,
    pub leveled_up: bool,
}

pub fn required_exp_for_level(level: u8) -> u16 {
    if level >= MANES_MAX_LEVEL {
        0
    } else {
        MANES_LEVEL_EXP[(level.saturating_sub(1)) as usize]
    }
}

/// EXP rewards are intentionally independent from the persistent NPC EXP.
/// They follow the supplied Manes grade ranges and let an active player reach
/// level 30 after clearing most of one physical zone, without making the early
/// levels grindy.
pub fn monster_survival_exp(npc_sid: u16) -> u16 {
    match npc_sid {
        10701..=10705 => 80,
        10706..=10709 => 140,
        10710..=10715 => 160,
        10716..=10719 => 260,
        10720..=10728 => 280,
        10729..=10732 => 450,
        10733 => 1_000,
        _ => 0,
    }
}

pub struct ManesSurvivalManager {
    spawns: RwLock<Vec<ManesSurvivalSpawnRow>>,
    registration_open: AtomicBool,
    active: AtomicBool,
    participants: DashSet<SessionId>,
    progress: DashMap<SessionId, ManesProgress>,
    unlocked_magic: DashMap<SessionId, DashSet<u32>>,
}

impl Default for ManesSurvivalManager {
    fn default() -> Self {
        Self {
            spawns: RwLock::new(Vec::new()),
            registration_open: AtomicBool::new(false),
            active: AtomicBool::new(false),
            participants: DashSet::new(),
            progress: DashMap::new(),
            unlocked_magic: DashMap::new(),
        }
    }
}

impl ManesSurvivalManager {
    pub fn set_spawns(&self, rows: Vec<ManesSurvivalSpawnRow>) -> anyhow::Result<()> {
        if rows.len() != 33 {
            anyhow::bail!("Manes Survival requires exactly 33 configured monster types; loaded {}", rows.len());
        }
        if rows.iter().filter(|r| r.npc_id == DARK_DRAGON_SID && r.boss_tier == 3).count() != 1 {
            anyhow::bail!("Manes Survival requires exactly one Dark Dragon configuration");
        }
        if rows.iter().any(|r| r.spawn_count <= 0 || r.spawn_x < 0 || r.spawn_z < 0 || r.spawn_range < 0) {
            anyhow::bail!("Manes Survival contains invalid spawn count or coordinates");
        }
        *self.spawns.write() = rows;
        Ok(())
    }

    pub fn is_active(&self) -> bool { self.active.load(Ordering::Acquire) }

    pub fn is_registration_open(&self) -> bool {
        self.registration_open.load(Ordering::Acquire)
    }

    pub fn open_registration(&self) -> bool {
        if self.is_active() {
            return false;
        }
        self.participants.clear();
        !self.registration_open.swap(true, Ordering::AcqRel)
    }

    pub fn register(&self, session_id: SessionId) -> bool {
        self.is_registration_open() && self.participants.insert(session_id)
    }

    pub fn unregister(&self, session_id: SessionId) -> bool {
        self.is_registration_open() && self.participants.remove(&session_id).is_some()
    }

    pub fn participant_count(&self) -> usize { self.participants.len() }

    pub fn participant_ids(&self) -> Vec<SessionId> {
        self.participants.iter().map(|entry| *entry.key()).collect()
    }

    pub fn active_participant_ids(&self, world: &WorldState) -> Vec<SessionId> {
        let mut ids: Vec<_> = self
            .participant_ids()
            .into_iter()
            .filter(|sid| world.get_character_info(*sid).is_some())
            .collect();
        ids.sort_unstable();
        ids
    }

    pub fn configured_count(&self) -> usize { self.spawns.read().len() }

    pub fn progress(&self, session_id: SessionId) -> Option<ManesProgress> {
        self.progress.get(&session_id).map(|entry| *entry.value())
    }

    /// Convert a MANES_MAGIC.tbl row into the real MAGIC table identifier.
    /// Rows 5901..5907, 6001..6008 and 6101..6108 map consecutively to
    /// 491330..491352 in the v2615 client tables.
    pub fn manes_magic_id(row_id: u16) -> Option<u32> {
        match row_id {
            5901..=5907 => Some(491_330 + u32::from(row_id - 5901)),
            6001..=6008 => Some(491_337 + u32::from(row_id - 6001)),
            6101..=6108 => Some(491_345 + u32::from(row_id - 6101)),
            _ => None,
        }
    }

    pub fn unlock_magic(&self, session_id: SessionId, row_id: u16) -> Option<u32> {
        let magic_id = Self::manes_magic_id(row_id)?;
        self.unlocked_magic.entry(session_id).or_default().insert(magic_id);
        Some(magic_id)
    }

    pub fn has_unlocked_magic(&self, session_id: SessionId, magic_id: u32) -> bool {
        self.unlocked_magic
            .get(&session_id)
            .map(|skills| skills.contains(&magic_id))
            .unwrap_or(false)
    }

    pub fn reset_progress(&self, session_id: SessionId) -> ManesProgress {
        let progress = ManesProgress {
            level: 1,
            exp: 0,
            max_exp: required_exp_for_level(1),
            leveled_up: false,
        };
        self.progress.insert(session_id, progress);
        self.unlocked_magic.remove(&session_id);
        progress
    }

    pub fn award_monster_exp(
        &self,
        session_id: SessionId,
        npc_sid: u16,
    ) -> Option<ManesProgress> {
        if !self.is_active() {
            return None;
        }
        let reward = monster_survival_exp(npc_sid);
        if reward == 0 {
            return None;
        }

        let mut entry = self.progress.entry(session_id).or_insert(ManesProgress {
            level: 1,
            exp: 0,
            max_exp: required_exp_for_level(1),
            leveled_up: false,
        });
        let mut state = *entry;
        state.leveled_up = false;

        if state.level < MANES_MAX_LEVEL {
            state.exp = state.exp.saturating_add(reward);
            while state.level < MANES_MAX_LEVEL && state.exp >= state.max_exp {
                state.exp -= state.max_exp;
                state.level += 1;
                state.leveled_up = true;
                state.max_exp = required_exp_for_level(state.level);
            }
            if state.level >= MANES_MAX_LEVEL {
                state.level = MANES_MAX_LEVEL;
                state.exp = 0;
                state.max_exp = 0;
            }
        }

        *entry = state;
        Some(state)
    }

    /// Start the event and place every online registrant on the nation-specific
    /// outer entry line. The client-confirmed registration countdown sends this
    /// transition once it expires; the GM command uses this same path.
    pub fn start_registered_event(&self, world: &WorldState) -> anyhow::Result<(usize, usize)> {
        let participants = self.active_participant_ids(world);
        if participants.len() < MIN_ACTIVE_PARTICIPANTS {
            anyhow::bail!(
                "Manes Survival requires at least {MIN_ACTIVE_PARTICIPANTS} active participant"
            );
        }

        let monster_count = self.start(world)?;
        if monster_count == 0 {
            return Ok((0, participants.len()));
        }

        if let Err(error) = self.place_participants(world, &participants) {
            self.stop(world);
            return Err(error);
        }

        Ok((monster_count, participants.len()))
    }

    fn place_participants(
        &self,
        world: &WorldState,
        participants: &[SessionId],
    ) -> anyhow::Result<()> {
        let mut karus_index = 0usize;
        let mut elmorad_index = 0usize;

        for (zone_index, sid) in participants.iter().copied().enumerate() {
            let character = world
                .get_character_info(sid)
                .ok_or_else(|| anyhow::anyhow!("registered participant {sid} is no longer online"))?;
            let nation = character.nation;
            let (start, end, nation_index) = match nation {
                crate::world::NATION_KARUS => {
                    let index = karus_index;
                    karus_index += 1;
                    (KARUS_ENTRY_START, KARUS_ENTRY_END, index)
                }
                crate::world::NATION_ELMORAD => {
                    let index = elmorad_index;
                    elmorad_index += 1;
                    (ELMORAD_ENTRY_START, ELMORAD_ENTRY_END, index)
                }
                _ => anyhow::bail!("registered participant {sid} has invalid nation {nation}"),
            };

            // Golden-ratio spacing is deterministic, avoids stacked users and
            // stays strictly between the user-supplied outer-ring endpoints.
            let fraction = (((nation_index + 1) as f32) * 0.618_034).fract();
            let x = start.0 + (end.0 - start.0) * fraction;
            let z = start.1 + (end.1 - start.1) * fraction;
            let zone_id = ZONES_MANES_SURVIVAL[zone_index % ZONES_MANES_SURVIVAL.len()];
            let zone = world
                .get_zone(zone_id)
                .ok_or_else(|| anyhow::anyhow!("Manes Survival zone {zone_id} is not loaded"))?;
            if !zone.is_valid_position(x, z) {
                anyhow::bail!(
                    "Manes Survival participant {sid} has invalid entry position in zone {zone_id}: ({x:.1}, {z:.1})"
                );
            }

            // The client keeps the registration countdown alive until this
            // exact event-state packet initialises CSurvival. Send it before
            // zone change so the first event NPC death cannot race ahead of
            // client initialisation.
            let progress = self.reset_progress(sid);
            let initial_max_exp = progress.max_exp;
            // Client reverse contract (sub_7113D0 -> sub_716B50): this
            // 1-based value selects the complete temporary Manes loadout.
            // Manes is a Chaos-style event: every participant must receive
            // the same skills, clothing and weapon regardless of their normal
            // class, nation, race or gender. Row 3 is the client-confirmed
            // canonical loadout used by the last known-good test.
            const UNIFORM_MANES_LOADOUT: u8 = 3;
            let survival_setting = UNIFORM_MANES_LOADOUT;
            world.send_to_session_owned(
                sid,
                crate::handler::survival::build_event_start(
                    survival_setting,
                    crate::handler::survival::EVENT_DURATION_SECONDS,
                    0,
                    initial_max_exp,
                    1,
                ),
            );

            crate::handler::zone_change::server_teleport_to_zone_force(
                world, sid, zone_id, x, z,
            );
        }

        Ok(())
    }

    /// Spawn the configured event population exactly once.
    pub fn start(&self, world: &WorldState) -> anyhow::Result<usize> {
        if self.active.swap(true, Ordering::AcqRel) {
            return Ok(0);
        }

        self.registration_open.store(false, Ordering::Release);

        let rows = self.spawns.read().clone();
        if rows.is_empty() {
            self.active.store(false, Ordering::Release);
            anyhow::bail!("Manes Survival spawn configuration is empty");
        }

        let mut total = 0usize;
        for zone_id in ZONES_MANES_SURVIVAL {
            let zone = match world.get_zone(zone_id) {
                Some(zone) => zone,
                None => {
                    self.stop(world);
                    anyhow::bail!("Manes Survival zone {zone_id} is not loaded");
                }
            };

            for row in &rows {
                let count = row.spawn_count as usize;
                let phase = (row.npc_id.rem_euclid(360) as f32).to_radians();
                let mut created = 0usize;

                for index in 0..count {
                    // Deterministic golden-angle scatter fills the configured local area
                    // without stacking every monster on a diagonal line.
                    let fraction = if count <= 1 {
                        0.0
                    } else {
                        ((index + 1) as f32 / count as f32).sqrt()
                    };
                    let angle = phase + index as f32 * 2.399_963_1;
                    let radius = row.spawn_range as f32 * fraction;
                    let x = row.spawn_x as f32 + angle.cos() * radius;
                    let z = row.spawn_z as f32 + angle.sin() * radius;

                    if !zone.is_valid_position(x, z) {
                        self.stop(world);
                        anyhow::bail!(
                            "Manes Survival zone {zone_id} NPC {} has out-of-map position ({x:.1}, {z:.1})",
                            row.npc_id
                        );
                    }

                    created += world
                        .spawn_event_npc_ex(
                            row.npc_id as u16,
                            true,
                            zone_id,
                            x,
                            z,
                            1,
                            MANES_EVENT_ROOM,
                            row.boss_tier as u8,
                        )
                        .len();
                }

                if created != count {
                    self.stop(world);
                    anyhow::bail!(
                        "Manes Survival zone {zone_id} failed to spawn NPC {}: expected {}, created {}",
                        row.npc_id,
                        count,
                        created
                    );
                }
                total += created;
            }
        }
        Ok(total)
    }

    /// Remove only runtime NPCs owned by the four Manes physical zones.
    pub fn stop(&self, world: &WorldState) {
        for zone_id in ZONES_MANES_SURVIVAL {
            world.despawn_room_npcs(zone_id, MANES_EVENT_ROOM);
        }
        self.active.store(false, Ordering::Release);
        self.registration_open.store(false, Ordering::Release);
        self.participants.clear();
        self.progress.clear();
        self.unlocked_magic.clear();
    }
}
