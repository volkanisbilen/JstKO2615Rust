//! Manes Survival PostgreSQL configuration rows.

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ManesSurvivalSpawnRow {
    pub id: i32,
    pub npc_id: i16,
    /// 1=low, 2=mid, 3=high, 4=Dark Dragon.
    pub grade: i16,
    /// 0=normal, 1=low boss, 2=castle boss, 3=Dark Dragon.
    pub boss_tier: i16,
    pub spawn_x: i32,
    pub spawn_z: i32,
    pub spawn_range: i16,
    pub spawn_count: i16,
    pub respawn_seconds: i16,
}
