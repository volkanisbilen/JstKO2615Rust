//! Manes Survival runtime monster lifecycle.
//!
//! Client contract: zone 96, WIZ_SURVIVAL (0xD0). Event monsters are runtime
//! NPCs and never belong in the global npc_spawn table.

use std::sync::atomic::{AtomicBool, Ordering};

use ko_db::models::ManesSurvivalSpawnRow;
use parking_lot::RwLock;

use crate::world::WorldState;

pub const ZONE_MANES_SURVIVAL: u16 = 96;
/// Dedicated event-room marker used to isolate and clean Manes runtime NPCs.
pub const MANES_EVENT_ROOM: u16 = 96;
pub const DARK_DRAGON_SID: i16 = 10733;

pub struct ManesSurvivalManager {
    spawns: RwLock<Vec<ManesSurvivalSpawnRow>>,
    active: AtomicBool,
}

impl Default for ManesSurvivalManager {
    fn default() -> Self {
        Self { spawns: RwLock::new(Vec::new()), active: AtomicBool::new(false) }
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
        if rows.iter().any(|r| r.spawn_count <= 0 || r.spawn_x < 0 || r.spawn_z < 0) {
            anyhow::bail!("Manes Survival contains invalid spawn count or coordinates");
        }
        *self.spawns.write() = rows;
        Ok(())
    }

    pub fn is_active(&self) -> bool { self.active.load(Ordering::Acquire) }

    pub fn configured_count(&self) -> usize { self.spawns.read().len() }

    /// Spawn the configured event population exactly once.
    pub fn start(&self, world: &WorldState) -> anyhow::Result<usize> {
        if self.active.swap(true, Ordering::AcqRel) {
            return Ok(0);
        }

        let zone = match world.get_zone(ZONE_MANES_SURVIVAL) {
            Some(zone) => zone,
            None => {
                self.active.store(false, Ordering::Release);
                anyhow::bail!("Manes Survival zone 96 is not loaded");
            }
        };

        let rows = self.spawns.read().clone();
        if rows.is_empty() {
            self.active.store(false, Ordering::Release);
            anyhow::bail!("Manes Survival spawn configuration is empty");
        }

        let mut total = 0usize;
        for row in &rows {
            let x = row.spawn_x as f32;
            let z = row.spawn_z as f32;
            if !zone.is_valid_position(x, z) {
                self.stop(world);
                anyhow::bail!("Manes Survival NPC {} has out-of-map coordinates ({}, {})", row.npc_id, row.spawn_x, row.spawn_z);
            }
            let ids = world.spawn_event_npc_ex(
                row.npc_id as u16,
                true,
                ZONE_MANES_SURVIVAL,
                x,
                z,
                row.spawn_count as u16,
                MANES_EVENT_ROOM,
                row.boss_tier as u8,
            );
            if ids.len() != row.spawn_count as usize {
                self.stop(world);
                anyhow::bail!("Manes Survival failed to spawn NPC {}: expected {}, created {}", row.npc_id, row.spawn_count, ids.len());
            }
            total += ids.len();
        }
        Ok(total)
    }

    /// Remove only Manes runtime NPCs; other zone-96 objects are untouched.
    pub fn stop(&self, world: &WorldState) {
        world.despawn_room_npcs(ZONE_MANES_SURVIVAL, MANES_EVENT_ROOM);
        self.active.store(false, Ordering::Release);
    }
}
