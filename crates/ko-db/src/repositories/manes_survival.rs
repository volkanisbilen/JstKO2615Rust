//! Manes Survival configuration repository.

use crate::models::manes_survival::ManesSurvivalSpawnRow;
use crate::DbPool;

pub struct ManesSurvivalRepository<'a> {
    pool: &'a DbPool,
}

impl<'a> ManesSurvivalRepository<'a> {
    pub fn new(pool: &'a DbPool) -> Self { Self { pool } }

    pub async fn load_spawns(&self) -> Result<Vec<ManesSurvivalSpawnRow>, sqlx::Error> {
        sqlx::query_as::<_, ManesSurvivalSpawnRow>(
            "SELECT id, npc_id, grade, boss_tier, spawn_x, spawn_z, \
             spawn_range, spawn_count, respawn_seconds \
             FROM manes_survival_spawn ORDER BY grade, boss_tier, id",
        )
        .fetch_all(self.pool)
        .await
    }
}
