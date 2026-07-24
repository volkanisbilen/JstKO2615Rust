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
const EVENT_START: u8 = 1;
const EVENT_SKILL_SELECT: u8 = 3;
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

/// Ask the v2615 client to open the Manes level-up selection UIF.
///
/// The client answers this three-byte notification with the same
/// `D0 02 03` category/operation request. Only after that request is received
/// may the server send the populated selection model.
pub fn build_skill_selection_open() -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_EVENT);
    pkt.write_u8(EVENT_SKILL_SELECT);
    pkt
}

/// Populate an already-open Manes level-up selection UIF.
pub fn build_skill_selection(level: u8) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_EVENT);
    pkt.write_u8(EVENT_SKILL_SELECT);
    pkt.write_u8(level);
    pkt.write_u16(1);
    pkt.write_i32(0);
    pkt.write_u8(3);
    write_skill_option(&mut pkt, 6101, "HP Increase 1", "Increase own HP by 100", 491345);
    write_skill_option(&mut pkt, 6001, "Attack Damage Increase 1", "Increase attack damage", 491337);
    write_skill_option(&mut pkt, 5901, "Reduce Attack Damage 1", "Reduce received damage", 491330);
    pkt
}

fn write_skill_option(pkt: &mut Packet, skill_id: u32, name: &str, description: &str, icon_id: i32) {
    pkt.write_u8(5);
    // v2615 sub_751500 reads the option identifier as a 32-bit value.
    // Writing u16 shifts every following string/value field by two bytes.
    pkt.write_u32(skill_id);
    pkt.write_string(name);
    pkt.write_string(description);
    pkt.write_i16(0);
    pkt.write_i32(icon_id);
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

    if category == CATEGORY_EVENT && operation == EVENT_SKILL_SELECT {
        let sid = session.session_id();
        let level = session
            .world()
            .manes_survival_manager
            .progress(sid)
            .map(|state| state.level)
            .unwrap_or(1);
        session.send_packet(&build_skill_selection(level)).await?;
        debug!(
            "[{}] Manes skill-selection options sent sid={} level={}",
            session.addr(),
            sid,
            level
        );
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
    fn skill_selection_open_is_header_only() {
        let packet = build_skill_selection_open();
        assert_eq!(packet.opcode, 0xD0);
        assert_eq!(packet.data, vec![0x02, 0x03]);
    }

    #[test]
    fn skill_selection_options_are_not_used_as_the_open_trigger() {
        let packet = build_skill_selection(2);
        assert_eq!(&packet.data[..2], &[0x02, 0x03]);
        assert!(packet.data.len() > build_skill_selection_open().data.len());
    }

    #[test]
    fn skill_selection_option_ids_are_v2615_u32_values() {
        let packet = build_skill_selection(4);
        // Header: category, operation, level, available point (u16),
        // selected skill (i32), option count. First option then starts with
        // group=5 and a little-endian u32 identifier.
        assert_eq!(&packet.data[..10], &[0x02, 0x03, 0x04, 0x01, 0x00, 0, 0, 0, 0, 0x03]);
        assert_eq!(&packet.data[10..15], &[0x05, 0xD5, 0x17, 0x00, 0x00]);
        assert_eq!(packet.data.len(), 180);
    }
}
