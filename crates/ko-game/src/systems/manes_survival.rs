//! Manes Survival runtime monster lifecycle.
//!
//! Client contract: Zones.tbl 570/580/590/600 map to server zones 57/58/59/60;
//! WIZ_SURVIVAL uses shared opcode byte 0xD0. Event monsters are runtime
//! NPCs and never belong in the global npc_spawn table.

use std::sync::atomic::{AtomicBool, Ordering};

use ko_db::models::ManesSurvivalSpawnRow;
use parking_lot::RwLock;

use crate::world::WorldState;

pub const ZONES_MANES_SURVIVAL: [u16; 4] = [57, 58, 59, 60];
/// Zones 57-60 are dedicated physical instances, so their NPCs share room 0
/// with players entering through the normal zone-change flow.
pub const MANES_EVENT_ROOM: u16 = 0;
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
        if rows.iter().any(|r| r.spawn_count <= 0 || r.spawn_x < 0 || r.spawn_z < 0 || r.spawn_range < 0) {
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
    }
}
