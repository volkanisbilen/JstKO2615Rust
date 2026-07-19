//! Manes Survival 2615 client protocol.
//!
//! Verified from the unpacked client dispatchers:
//! - S2C D3 01: open registration UI
//! - S2C D3 02 u8 result/nation: registration result
//! - S2C D3 03 u32 elapsed, u32 Karus count, u32 El Morad count: status
//! - C2S D0 01 02 u16 action: 1 apply, 2 cancel
//!
//! Opcode 0xD0 is shared with the in-event Survival gameplay dispatcher.
//! Registration responses must use the separate 0xD3 client dispatcher.

use std::sync::Arc;

use ko_protocol::{Packet, PacketReader};
use tracing::{debug, warn};

use crate::session::{ClientSession, SessionState};

const WIZ_SURVIVAL_GAMEPLAY: u8 = 0xD0;
const WIZ_SURVIVAL_REGISTRATION: u8 = 0xD3;
const CATEGORY_REGISTRATION: u8 = 1;
const REG_OPEN: u8 = 1;
const REG_APPLY: u8 = 2;
const REG_STATUS: u8 = 3;
const ACTION_APPLY: u16 = 1;
const ACTION_CANCEL: u16 = 2;

pub fn build_registration_open() -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL_REGISTRATION);
    pkt.write_u8(REG_OPEN);
    pkt
}

pub fn build_registration_result(result: u8) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL_REGISTRATION);
    pkt.write_u8(REG_APPLY);
    pkt.write_u8(result);
    pkt
}

pub fn build_registration_status(elapsed_seconds: u32, karus: u32, el_morad: u32) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL_REGISTRATION);
    pkt.write_u8(REG_STATUS);
    pkt.write_u32(elapsed_seconds);
    pkt.write_u32(karus);
    pkt.write_u32(el_morad);
    pkt
}

pub fn broadcast_registration_status(world: &crate::world::WorldState, elapsed_seconds: u32) {
    let mut karus = 0u32;
    let mut el_morad = 0u32;
    for sid in world.manes_survival_manager.participant_ids() {
        match world.get_character_info(sid).map(|ch| ch.nation) {
            Some(1) => karus += 1,
            Some(2) => el_morad += 1,
            _ => {}
        }
    }

    let packet = Arc::new(build_registration_status(elapsed_seconds, karus, el_morad));
    for sid in world.get_in_game_session_ids() {
        world.send_to_session_arc(sid, Arc::clone(&packet));
    }
}

pub async fn handle(session: &mut ClientSession, pkt: Packet) -> anyhow::Result<()> {
    if session.state() != SessionState::InGame {
        return Ok(());
    }

    debug_assert_eq!(pkt.opcode, WIZ_SURVIVAL_GAMEPLAY);

    let mut reader = PacketReader::new(&pkt.data);
    let category = reader.read_u8().unwrap_or(0);
    let operation = reader.read_u8().unwrap_or(0);

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
            let nation = world.get_character_info(sid).map(|ch| ch.nation).unwrap_or(0);
            let result = if manager.register(sid) { nation } else { 0 };
            session.send_packet(&build_registration_result(result)).await?;
            broadcast_registration_status(&world, 0);
            debug!(
                "[{}] Manes registration apply sid={} result={} participants={}",
                session.addr(), sid, result, manager.participant_count()
            );
        }
        ACTION_CANCEL => {
            manager.unregister(sid);
            session.send_packet(&build_registration_result(0)).await?;
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
