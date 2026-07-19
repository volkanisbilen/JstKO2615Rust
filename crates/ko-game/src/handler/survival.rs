//! Manes Survival 2615 client protocol.
//!
//! Byte contract verified against the unpacked client CSurvival dispatcher
//! (sub_716A10 -> category 1 -> sub_711080):
//! - S2C D0 01 01 u16 remaining_seconds u16 participant_count: open/refresh entry UI
//! - C2S D0 01 02 u16 action: 1 apply, 2 cancel
//! - S2C D0 01 02 i16 result [u16 participant_count when result=1]
//!
//! The similarly shaped 0xD3 handler belongs to the Ronark-war UI and must not
//! be used for Manes Survival registration.

use std::sync::Arc;

use ko_protocol::{Packet, PacketReader};
use tracing::{debug, warn};

use crate::session::{ClientSession, SessionState};

const WIZ_SURVIVAL: u8 = 0xD0;
const CATEGORY_REGISTRATION: u8 = 1;
const REG_OPEN: u8 = 1;
const REG_APPLY: u8 = 2;
const ACTION_APPLY: u16 = 1;
const ACTION_CANCEL: u16 = 2;
const CATEGORY_EVENT: u8 = 2;
const EVENT_COUNTDOWN_COMPLETE: u8 = 3;
pub const REGISTRATION_DURATION_SECONDS: u16 = 600;

pub fn build_registration_open(remaining_seconds: u16, participant_count: u16) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_REGISTRATION);
    pkt.write_u8(REG_OPEN);
    pkt.write_u16(remaining_seconds);
    pkt.write_u16(participant_count);
    pkt
}

pub fn build_registration_result(result: i16, participant_count: u16) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_REGISTRATION);
    pkt.write_u8(REG_APPLY);
    pkt.write_u16(result as u16);
    if result == 1 {
        pkt.write_u16(participant_count);
    }
    pkt
}

pub fn broadcast_registration_status(world: &crate::world::WorldState, elapsed_seconds: u32) {
    let remaining = REGISTRATION_DURATION_SECONDS
        .saturating_sub(elapsed_seconds.min(u16::MAX as u32) as u16);
    let participants = world
        .manes_survival_manager
        .participant_count()
        .min(u16::MAX as usize) as u16;
    let packet = Arc::new(build_registration_open(remaining, participants));
    for sid in world.get_in_game_session_ids() {
        world.send_to_session_arc(sid, Arc::clone(&packet));
    }
}

pub async fn handle(session: &mut ClientSession, pkt: Packet) -> anyhow::Result<()> {
    if session.state() != SessionState::InGame {
        return Ok(());
    }

    let mut reader = PacketReader::new(&pkt.data);
    let category = reader.read_u8().unwrap_or(0);
    let operation = reader.read_u8().unwrap_or(0);

    // Verified from the 2615 client trace: when the registration timer reaches
    // zero it sends D0 02 03. Only the first client signal performs the global
    // transition; later signals observe the already-active manager and no-op.
    if category == CATEGORY_EVENT && operation == EVENT_COUNTDOWN_COMPLETE {
        let world = session.world().clone();
        if !world.manes_survival_manager.is_active() {
            match world.manes_survival_manager.start_registered_event(&world) {
                Ok((monsters, participants)) => debug!(
                    "[{}] Manes countdown completed: participants={} monsters={}",
                    session.addr(), participants, monsters
                ),
                Err(error) => warn!(
                    "[{}] Manes automatic start rejected: {error:#}",
                    session.addr()
                ),
            }
        }
        return Ok(());
    }

    if category != CATEGORY_REGISTRATION || operation != REG_APPLY {
        debug!(
            "[{}] WIZ_SURVIVAL unsupported C2S category={} operation={} remaining={}",
            session.addr(),
            category,
            operation,
            reader.remaining()
        );
        return Ok(());
    }

    let action = reader.read_u16().unwrap_or(0);
    let sid = session.session_id();
    let world = session.world().clone();
    let manager = &world.manes_survival_manager;

    match action {
        ACTION_APPLY => {
            let registered = manager.register(sid);
            let participants = manager.participant_count().min(u16::MAX as usize) as u16;
            let result = if registered { 1 } else { -1 };
            session
                .send_packet(&build_registration_result(result, participants))
                .await?;
            broadcast_registration_status(&world, 0);
            debug!(
                "[{}] Manes registration apply sid={} result={} participants={}",
                session.addr(), sid, result, manager.participant_count()
            );
        }
        ACTION_CANCEL => {
            manager.unregister(sid);
            let participants = manager.participant_count().min(u16::MAX as usize) as u16;
            session
                .send_packet(&build_registration_result(2, participants))
                .await?;
            broadcast_registration_status(&world, 0);
            debug!(
                "[{}] Manes registration cancel sid={} participants={}",
                session.addr(), sid, manager.participant_count()
            );
        }
        _ => warn!(
            "[{}] WIZ_SURVIVAL invalid registration action={}",
            session.addr(), action
        ),
    }

    Ok(())
}
