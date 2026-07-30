//! Manes Survival 2615 client protocol.
//!
//! Byte contract verified against the unpacked client CSurvival dispatcher
//! (`sub_716A10`):
//! - S2C D0 01 01 u16 remaining_seconds u16 participant_count: open/refresh entry UI
//! - C2S D0 01 02 u16 action: 1 apply, 2 cancel
//! - S2C D0 01 02 i16 result [u16 participant_count when result=1]
//! - S2C D0 06 01 u8 first_count [u16 skill_id] u8 second_count
//!   [u16 skill_id]: load `MANES_MAGIC.tbl` rows and open the skill-choice UI
//! - C2S D0 06 02 u8 list_type u16 manes_magic_id: submit a selection
//!   (`list_type=1` skill, `list_type=2` potion)
//! - S2C D0 06 02 u8 list_type i16 result: complete that selection (`result=1`)
//! - S2C D0 06 04 u8 count [u16 skill_id]: refresh only the skill list
//! - C2S D0 06 05 u8 state [u16 skill_id when state != 2]: complete selection
//! - S2C D0 06 05 i16 result: accept selection and close the choice UI
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
const SELECTION_SUBMIT: u8 = 2;
const SKILL_UPDATE: u8 = 4;
const SKILL_COMPLETE: u8 = 5;
const SKILL_RESULT_FAILURE: u8 = 0;
const SELECTION_LIST_SKILL: u8 = 1;
const SELECTION_LIST_POTION: u8 = 2;
const SURVIVAL_SKILL_BRANCHES: [(u16, u16); 3] = [(6100, 8), (6000, 8), (5900, 7)];
const SURVIVAL_POTION_CHOICES: [u16; 2] = [6201, 6301];
const POTION_PURCHASE_COUNT: u16 = 10;
const POTION_PURCHASE_PRICE: u32 = 3_000;
const MANES_HP_POTION_ITEM: u32 = 978_023_000;
const MANES_MP_POTION_ITEM: u32 = 978_024_000;
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
pub fn skill_choices_for_level(survival_level: u8) -> [u16; 3] {
    let tier = u16::from(survival_level.max(1));
    SURVIVAL_SKILL_BRANCHES.map(|(branch, max_tier)| branch + tier.min(max_tier))
}

pub fn build_skill_selection_open(survival_level: u8) -> Packet {
    let skill_choices = skill_choices_for_level(survival_level);
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_SKILL);
    pkt.write_u8(SKILL_OPEN);
    pkt.write_u8(skill_choices.len() as u8);
    for skill_id in skill_choices {
        pkt.write_u16(skill_id);
    }
    pkt.write_u8(SURVIVAL_POTION_CHOICES.len() as u8);
    for potion_id in SURVIVAL_POTION_CHOICES {
        pkt.write_u16(potion_id);
    }
    pkt
}

/// Refresh only the level-dependent Manes skill choices.
///
/// The unpacked v2615 client routes operation 4 to the skill-list updater.
/// Unlike operation 1, it does not rebuild the potion selector or clear the
/// temporary HP/MP quick slots.
pub fn build_skill_selection_update(survival_level: u8) -> Packet {
    let skill_choices = skill_choices_for_level(survival_level);
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_SKILL);
    pkt.write_u8(SKILL_UPDATE);
    pkt.write_u8(skill_choices.len() as u8);
    for skill_id in skill_choices {
        pkt.write_u16(skill_id);
    }
    pkt
}

pub fn build_skill_selection_for_level(survival_level: u8) -> Packet {
    if survival_level <= 2 {
        build_skill_selection_open(survival_level)
    } else {
        build_skill_selection_update(survival_level)
    }
}

/// Confirm a v2615 Manes skill selection.
///
/// `sub_714AB0`, operation 5, reads a signed 16-bit result code. Returning a
/// MANES_MAGIC row ID here is interpreted as an error and leaves Complete in a
/// retry loop.
pub fn build_skill_selection_result(result: i16) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_SKILL);
    pkt.write_u8(SKILL_COMPLETE);
    pkt.write_u16(result as u16);
    pkt
}

/// Complete a Manes skill/potion list selection.
///
/// Verified against `sub_714AB0`, operation 2. The client reads the selection
/// list type followed by a signed result code. Result 1 commits the selected
/// `MANES_MAGIC` row in the corresponding skill or potion UI.
pub fn build_selection_submit_result(list_type: u8, result: i16) -> Packet {
    let mut pkt = Packet::new(WIZ_SURVIVAL);
    pkt.write_u8(CATEGORY_SKILL);
    pkt.write_u8(SELECTION_SUBMIT);
    pkt.write_u8(list_type);
    pkt.write_u16(result as u16);
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

    if category == CATEGORY_SKILL && operation == SELECTION_SUBMIT {
        let list_type = reader.read_u8().unwrap_or(SKILL_RESULT_FAILURE);
        let manes_magic_id = reader.read_u16().unwrap_or(0);
        let no_trailing_data = reader.remaining() == 0;

        // The main skill list uses the same operation as the potion list.
        // The v2615 client commits the selected MANES_MAGIC row to its skill
        // bar only after receiving `D0 06 02 01 01 00`.
        if list_type == SELECTION_LIST_SKILL {
            let survival_level = session
                .world()
                .manes_survival_manager
                .progress(session.session_id())
                .map(|progress| progress.level)
                .unwrap_or(1);
            let offered_choices = skill_choices_for_level(survival_level);
            let valid = no_trailing_data && offered_choices.contains(&manes_magic_id);
            session
                .send_packet(&build_selection_submit_result(
                    list_type,
                    if valid { 1 } else { 0 },
                ))
                .await?;
            if valid {
                let magic_id = session
                    .world()
                    .manes_survival_manager
                    .unlock_magic(session.session_id(), manes_magic_id)
                    .expect("validated MANES_MAGIC row must map to MAGIC");
                debug!(
                    "[{}] Manes skill list selection accepted sid={} manes_magic_id={} magic_id={}",
                    session.addr(),
                    session.session_id(),
                    manes_magic_id,
                    magic_id
                );
            } else {
                warn!(
                    "[{}] Manes skill list selection rejected sid={} list_type={} manes_magic_id={} remaining={}",
                    session.addr(),
                    session.session_id(),
                    list_type,
                    manes_magic_id,
                    reader.remaining()
                );
            }
            return Ok(());
        }

        let item_id = match manes_magic_id {
            6201 => Some(MANES_HP_POTION_ITEM),
            6301 => Some(MANES_MP_POTION_ITEM),
            _ => None,
        };
        let sid = session.session_id();
        let world = session.world().clone();
        let valid_request = list_type == SELECTION_LIST_POTION
            && no_trailing_data
            && item_id.is_some();
        let enough_gold = world
            .get_character_info(sid)
            .map(|character| character.gold >= POTION_PURCHASE_PRICE)
            .unwrap_or(false);

        if valid_request
            && enough_gold
            && world.check_weight(sid, item_id.unwrap(), POTION_PURCHASE_COUNT)
            && world.gold_lose(sid, POTION_PURCHASE_PRICE)
        {
            let item_id = item_id.unwrap();
            if world.give_item(sid, item_id, POTION_PURCHASE_COUNT) {
                session
                    .send_packet(&build_selection_submit_result(list_type, 1))
                    .await?;
                debug!(
                    "[{}] Manes potion purchased sid={} manes_magic_id={} item_id={} count={} price={}",
                    session.addr(),
                    sid,
                    manes_magic_id,
                    item_id,
                    POTION_PURCHASE_COUNT,
                    POTION_PURCHASE_PRICE
                );
            } else {
                world.gold_gain(sid, POTION_PURCHASE_PRICE);
                session
                    .send_packet(&build_selection_submit_result(list_type, 0))
                    .await?;
                warn!(
                    "[{}] Manes potion purchase rolled back sid={} manes_magic_id={} item_id={}",
                    session.addr(),
                    sid,
                    manes_magic_id,
                    item_id
                );
            }
        } else {
            session
                .send_packet(&build_selection_submit_result(list_type, 0))
                .await?;
            warn!(
                "[{}] Manes potion purchase rejected sid={} list_type={} manes_magic_id={} remaining={} enough_gold={}",
                session.addr(),
                sid,
                list_type,
                manes_magic_id,
                reader.remaining(),
                enough_gold
            );
        }
        return Ok(());
    }

    if category == CATEGORY_SKILL && operation == SKILL_COMPLETE {
        let requested_state = reader.read_u8().unwrap_or(SKILL_RESULT_FAILURE);
        let skill_id = reader.read_u16().unwrap_or(0);
        let survival_level = session
            .world()
            .manes_survival_manager
            .progress(session.session_id())
            .map(|progress| progress.level)
            .unwrap_or(1);
        let valid = requested_state != SKILL_RESULT_FAILURE
            && reader.remaining() == 0
            && skill_choices_for_level(survival_level).contains(&skill_id);
        if valid {
            session
                .send_packet(&build_skill_selection_result(1))
                .await?;
            debug!(
                "[{}] Manes skill selection completed sid={} skill_id={}",
                session.addr(),
                session.session_id(),
                skill_id
            );
        } else {
            warn!(
                "[{}] Manes skill selection rejected sid={} state={} skill_id={} remaining={}",
                session.addr(),
                session.session_id(),
                requested_state,
                skill_id,
                reader.remaining()
            );
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
    fn skill_selection_result_matches_v2615_client_contract() {
        let packet = build_skill_selection_result(1);
        assert_eq!(packet.opcode, 0xD0);
        assert_eq!(packet.data, vec![0x06, 0x05, 0x01, 0x00]);
    }

    #[test]
    fn skill_selection_submit_result_matches_v2615_client_contract() {
        let packet = build_selection_submit_result(SELECTION_LIST_SKILL, 1);
        assert_eq!(packet.opcode, 0xD0);
        assert_eq!(packet.data, vec![0x06, 0x02, 0x01, 0x01, 0x00]);
    }

    #[test]
    fn potion_selection_submit_result_matches_v2615_client_contract() {
        let packet = build_selection_submit_result(SELECTION_LIST_POTION, 1);
        assert_eq!(packet.opcode, 0xD0);
        assert_eq!(packet.data, vec![0x06, 0x02, 0x02, 0x01, 0x00]);
    }

    #[test]
    fn skill_selection_open_matches_v2615_client_contract() {
        let packet = build_skill_selection_open(1);
        assert_eq!(packet.opcode, 0xD0);
        assert_eq!(
            packet.data,
            vec![
                0x06, 0x01, 0x03, 0xD5, 0x17, 0x71, 0x17, 0x0D, 0x17, 0x02, 0x39, 0x18, 0x9D,
                0x18
            ]
        );
    }

    #[test]
    fn later_skill_selection_uses_update_operation() {
        let packet = build_skill_selection_for_level(3);
        assert_eq!(
            packet.data,
            vec![0x06, 0x04, 0x03, 0xD7, 0x17, 0x73, 0x17, 0x0F, 0x17]
        );
    }

    #[test]
    fn skill_choices_advance_with_survival_level() {
        assert_eq!(skill_choices_for_level(1), [6101, 6001, 5901]);
        assert_eq!(skill_choices_for_level(2), [6102, 6002, 5902]);
        assert_eq!(skill_choices_for_level(4), [6104, 6004, 5904]);
        assert_eq!(skill_choices_for_level(5), [6105, 6005, 5905]);
        assert_eq!(skill_choices_for_level(7), [6107, 6007, 5907]);
        assert_eq!(skill_choices_for_level(8), [6108, 6008, 5907]);
        assert_eq!(skill_choices_for_level(30), [6108, 6008, 5907]);
    }
}
