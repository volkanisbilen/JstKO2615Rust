//! Manes Survival 2615 client protocol (shared main opcode byte 0xD0).
//!
//! Verified from the unpacked client CSurvival dispatcher:
//! - S2C D0 01 01: open registration UI
//! - C2S D0 01 02 u16 action: 1 apply, 2 cancel
//! - S2C D0 01 02 u8 result/nation
//! - S2C D0 01 03 u32 elapsed, u32 Karus count, u32 El Morad count

use ko_protocol::{Packet, PacketReader};
use tracing::{debug, warn};

use crate::session::{ClientSession, SessionState};

const WIZ_SURVIVAL: u8 = 0xD0;
const CATEGORY_REGISTRATION: u8 = 1;
const REG_OPEN: u8 = 1;
const REG_APPLY: u8 = 2;
const REG_STATUS: u8 = 3;
const ACTION_APPLY: u16 = 1;
const ACTION_CANCEL: u16 = 2;

pub fn build_registration_open() -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_REGISTRATION);
    pkt.write_u8(REG_OPEN);
    pkt
}

pub fn build_registration_result(result: u8) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_REGISTRATION);
    pkt.write_u8(REG_APPLY);
    pkt.write_u8(result);
    pkt
}

pub fn build_registration_status(elapsed_seconds: u32, karus: u32, el_morad: u32) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_REGISTRATION);
    pkt.write_u8(REG_STATUS);
    pkt.write_u32(elapsed_seconds);
    pkt.write_u32(karus);
    pkt.write_u32(el_morad);
    pkt
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
            let nation = world.get_character_info(sid).map(|ch| ch.nation).unwrap_or(0);
            let result = if manager.register(sid) { nation } else { 0 };
            session.send_packet(&build_registration_result(result)).await?;
            debug!(
                "[{}] Manes registration apply sid={} result={} participants={}",
                session.addr(), sid, result, manager.participant_count()
            );
        }
        ACTION_CANCEL => {
            manager.unregister(sid);
            session.send_packet(&build_registration_result(0)).await?;
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
