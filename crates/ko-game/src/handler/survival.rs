//! Manes Survival 2615 client protocol.
//!
//! Byte contract verified against the unpacked client CSurvival dispatcher
//! (`sub_716A10`):
//! - S2C D0 01 01 u16 remaining_seconds u16 participant_count: open/refresh entry UI
//! - C2S D0 01 02 u16 action: 1 apply, 2 cancel
//! - S2C D0 01 02 i16 result [u16 participant_count when result=1]
//! - S2C D0 06 01 u8 first_count [u16 skill_id] u8 second_count
//!   [u16 skill_id]: load `MANES_MAGIC.tbl` rows and open the skill-choice UI
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
const EVENT_START: u8 = 1;
const CATEGORY_SKILL: u8 = 6;
const SKILL_OPEN: u8 = 1;
const SURVIVAL_SKILL_CHOICES: [u16; 3] = [6101, 6001, 5901];
pub const REGISTRATION_DURATION_SECONDS: u16 = 600;
pub const EVENT_DURATION_SECONDS: u16 = 1_200;

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


/// Initialise the v2615 Manes Survival client state.
///
/// Verified against `sub_716A10 -> sub_7113D0`, operation 1:
/// `D0 02 01 u8 survival_setting u16 seconds u16 exp u16 max_exp u8 level`.
pub fn build_event_start(
    survival_setting: u8,
    remaining_seconds: u16,
    survival_exp: u16,
    survival_max_exp: u16,
    survival_level: u8,
) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_EVENT);
    pkt.write_u8(EVENT_START);
    pkt.write_u8(survival_setting);
    pkt.write_u16(remaining_seconds);
    pkt.write_u16(survival_exp);
    pkt.write_u16(survival_max_exp);
    pkt.write_u8(survival_level);
    pkt
}

/// Open the v2615 Manes skill-choice window.
///
/// Verified against `sub_716A10 -> sub_714AB0`, operation 1. The client reads
/// two counted lists of `u16` MANES_MAGIC identifiers, resolves the rows via
/// `sub_710140`, then calls `sub_5037D0 -> sub_75C520` to populate and show the
/// choice UI. Names, descriptions, and icons are client table data and are not
/// part of this packet.
pub fn build_skill_selection_open() -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_SKILL);
    pkt.write_u8(SKILL_OPEN);
    pkt.write_u8(SURVIVAL_SKILL_CHOICES.len() as u8);
    for skill_id in SURVIVAL_SKILL_CHOICES {
        pkt.write_u16(skill_id);
    }
    // The client contract always contains a second counted list. There are no
    // secondary choices in this level-up offer.
    pkt.write_u8(0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_start_matches_v2615_client_contract() {
        let packet = build_event_start(3, 1_200, 0, 200, 1);
        assert_eq!(packet.opcode, 0xD0);
        assert_eq!(
            packet.data,
            vec![0x02, 0x01, 0x03, 0xB0, 0x04, 0x00, 0x00, 0xC8, 0x00, 0x01]
        );
    }

    #[test]
    fn skill_selection_open_matches_v2615_client_contract() {
        let packet = build_skill_selection_open();
        assert_eq!(packet.opcode, 0xD0);
        assert_eq!(
            packet.data,
            vec![0x06, 0x01, 0x03, 0xD5, 0x17, 0x71, 0x17, 0x0D, 0x17, 0x00]
        );
    }
}
