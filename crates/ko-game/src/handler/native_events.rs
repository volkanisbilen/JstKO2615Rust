//! Native event panels used by the verified v2615 client.
//!
//! Opcode families:
//! - `0x9C`: Event hub (`0xF0`) and Roulette (`6=open, 7=spin, 8=reveal, 9=history`)
//! - `0xCC`: Jigsaw selector `1`, Coin selector `2`
//! - `0xCF`: Knight Marble (`1=open, 2=roll, 3..5=panel actions`)

use chrono::{DateTime, Datelike, Utc};
use ko_db::models::native_events::{NativeJigsawState, NativeRoulettePending};
use ko_db::repositories::native_events::NativeEventsRepository;
use ko_protocol::{Opcode, Packet, PacketReader};
use rand::Rng;
use tracing::{debug, info, warn};

use crate::handler::knight_cash;
use crate::session::{ClientSession, SessionState};

const ROULETTE_FREE_TYPE: i32 = 1;
const ROULETTE_KC_TYPE: i32 = 2;
const ROULETTE_KC_COST: u32 = 350;

const EVENT_HUB_SUB: u8 = 0xF0;
const EVENT_HUB_COIN: u8 = 0;
const EVENT_HUB_ATTENDANCE: u8 = 1;
// The hub advertises Attendance as list id 1, but the v2615 client sends
// selector 4 when that row is clicked.
const EVENT_HUB_ATTENDANCE_SELECT: u8 = 4;
const EVENT_HUB_ROULETTE: u8 = 2;
const EVENT_HUB_JIGSAW: u8 = 3;
const EVENT_HUB_MARBLE: u8 = 5;

// CUIAttendanceCheck does not treat the i32 in each calendar entry as an item
// number.  The v2615 client searches its 28 in-memory slot records by this
// value, then resolves the actual reward through Attendance.tbl.  Daily rows
// are keyed 1..=25 and cumulative rows are keyed 101..=103.
fn attendance_daily_slot_key(day: usize) -> i32 {
    day as i32 + 1
}

fn attendance_cumulative_slot_key(index: usize) -> i32 {
    index as i32 + 101
}

fn character_name(session: &ClientSession) -> Option<String> {
    session
        .world()
        .get_character_info(session.session_id())
        .map(|c| c.name.clone())
        .filter(|v| !v.is_empty())
}

fn native_type(client_type: i32) -> i16 {
    if client_type == ROULETTE_KC_TYPE {
        8
    } else if client_type == ROULETTE_FREE_TYPE {
        7
    } else {
        7
    }
}

fn event_unavailable(opcode: u8, selector: u8) -> Packet {
    let mut pkt = Packet::new(opcode);
    pkt.write_u8(selector);
    pkt.write_i32(0);
    pkt
}

/// Seconds remaining in the current monthly attendance period.
///
/// The v2615 client uses this value for the panel countdown and treats zero as
/// an expired/unavailable calendar. The database progress resets by calendar
/// month, so the wire value must expire at the same boundary.
fn attendance_seconds_remaining(now: DateTime<Utc>) -> i32 {
    let (year, month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };

    let next_period = chrono::NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date_time| date_time.and_utc().timestamp())
        .unwrap_or_else(|| now.timestamp() + 1);

    (next_period - now.timestamp()).clamp(1, i32::MAX as i64) as i32
}

pub async fn handle_roulette(session: &mut ClientSession, pkt: Packet) -> anyhow::Result<()> {
    if session.state() != SessionState::InGame { return Ok(()); }
    let mut reader = PacketReader::new(&pkt.data);
    let sub = reader.read_u8().unwrap_or(0);
    let pool = session.pool().clone();
    let repo = NativeEventsRepository::new(&pool);
    if sub == EVENT_HUB_SUB {
        return native_event_hub_open(session, &repo).await;
    }
    if matches!(
        sub,
        EVENT_HUB_COIN
            | EVENT_HUB_ATTENDANCE
            | EVENT_HUB_ATTENDANCE_SELECT
            | EVENT_HUB_ROULETTE
            | EVENT_HUB_JIGSAW
            | EVENT_HUB_MARBLE
    ) {
        return native_event_hub_select(session, &repo, sub).await;
    }
    if !repo.is_active("roulette").await.unwrap_or(false) {
        let response = event_unavailable(Opcode::WizContinousPacketData as u8, sub);
        session.send_packet(&response).await?;
        return Ok(());
    }
    let Some(name) = character_name(session) else { return Ok(()); };
    match sub {
        6 => roulette_open(session, &repo).await,
        7 => roulette_spin(session, &repo, &name, reader.read_i32().unwrap_or(1)).await,
        8 => roulette_reveal(session, &repo, &name, reader.read_i32().unwrap_or(1)).await,
        9 => roulette_history(session, &repo, &name, reader.read_i32().unwrap_or(1)).await,
        _ => { debug!("[{}] native roulette unknown sub={sub}", session.addr()); Ok(()) }
    }
}

/// Reply to the star-button confirmation request.
///
/// The v2615 client treats `0xF0` as signed `-16` and dispatches it to
/// `CUIEventWebSelect::ReceiveMessage` (`0xAEB8C0`).  The verified wire format
/// after the subcommand is `i16 count`, followed by `u8 event_id, u8 enabled`
/// pairs. Disabled entries must be omitted: the client only adds pairs whose
/// enabled byte is non-zero to its selection list.
async fn native_event_hub_open(
    session: &mut ClientSession,
    repo: &NativeEventsRepository<'_>,
) -> anyhow::Result<()> {
    // The client renders entries in the order supplied by the server.
    let mut active = Vec::new();
    let candidates = [
        (EVENT_HUB_ATTENDANCE, "attendance"),
        (EVENT_HUB_ROULETTE, "roulette"),
        (EVENT_HUB_JIGSAW, "jigsaw"),
        (EVENT_HUB_COIN, "coin"),
        (EVENT_HUB_MARBLE, "marble"),
    ];
    for (event_id, event_key) in candidates {
        if repo.is_active(event_key).await.unwrap_or(false) {
            active.push((event_id, 1u8));
        }
    }

    let out = native_event_hub_packet(&active);
    session.send_packet(&out).await?;
    info!(
        "[{}] native event hub opened: active_ids={:?}",
        session.addr(),
        active.iter().map(|(id, _)| *id).collect::<Vec<_>>()
    );
    Ok(())
}

/// Dispatch a selection made in the star-button event list to the native
/// panel handler that already owns that event's wire contract.
async fn native_event_hub_select(
    session: &mut ClientSession,
    repo: &NativeEventsRepository<'_>,
    event_id: u8,
) -> anyhow::Result<()> {
    let event_key = match event_id {
        EVENT_HUB_ATTENDANCE | EVENT_HUB_ATTENDANCE_SELECT => "attendance",
        EVENT_HUB_ROULETTE => "roulette",
        EVENT_HUB_JIGSAW => "jigsaw",
        EVENT_HUB_COIN => "coin",
        EVENT_HUB_MARBLE => "marble",
        _ => return Ok(()),
    };

    if !repo.is_active(event_key).await.unwrap_or(false) {
        let response = event_unavailable(Opcode::WizContinousPacketData as u8, event_id);
        session.send_packet(&response).await?;
        return Ok(());
    }

    let result = match event_id {
        EVENT_HUB_ATTENDANCE | EVENT_HUB_ATTENDANCE_SELECT => {
            attendance_open(session).await
        }
        EVENT_HUB_ROULETTE => roulette_open(session, repo).await,
        EVENT_HUB_JIGSAW => {
            let Some(name) = character_name(session) else { return Ok(()); };
            jigsaw_open(session, repo, &name).await
        }
        EVENT_HUB_COIN => {
            let Some(name) = character_name(session) else { return Ok(()); };
            coin_open(session, repo, &name).await
        }
        EVENT_HUB_MARBLE => {
            let Some(name) = character_name(session) else { return Ok(()); };
            marble_open(session, repo, &name).await
        }
        _ => Ok(()),
    };

    info!(
        "[{}] native event hub selected: id={} key={}",
        session.addr(), event_id, event_key
    );
    result
}

/// Open the v2615 native Attendance panel.
///
/// The unpacked client dispatches this UI through WIZ_CONTINOUS_PACKET_DATA
/// (0x9C), not WIZ_ATTENDANCE (0xB7):
///
/// `[0x9C][outer=4][i32 error][i32 result][calendar state...]`
///
/// `outer=4` selects `CUIAttendanceCheck` at UI-manager offset `+0x66C`;
/// `error=0, result=1` loads the complete calendar and shows it.
async fn attendance_open(session: &mut ClientSession) -> anyhow::Result<()> {
    const TOTAL_DAYS: usize = 25;

    // The native v2615 panel does not send the legacy WIZ_ATTENDANCE claim
    // request. Claim today's next sequential reward on open; the shared claim
    // routine enforces one reward per calendar day and inventory capacity.
    let granted = crate::handler::attendance::claim_for_native_open(session).await?;

    let pool = session.pool().clone();
    let repo = ko_db::repositories::daily_reward::DailyRewardRepository::new(&pool);
    let rewards = repo.load_all().await.unwrap_or_else(|e| {
        warn!("[{}] native attendance load_all DB error: {e}", session.addr());
        Vec::new()
    });

    let Some(name) = character_name(session) else {
        return Ok(());
    };
    let progress = repo.load_user_progress(&name).await.unwrap_or_else(|e| {
        warn!(
            "[{}] native attendance load_user_progress DB error: {e}",
            session.addr()
        );
        Vec::new()
    });
    let cumulative = repo.load_cumulative().await.unwrap_or_else(|e| {
        warn!(
            "[{}] native attendance load_cumulative DB error: {e}",
            session.addr()
        );
        None
    });

    let mut claimed = [false; TOTAL_DAYS];
    for row in &progress {
        let index = row.day_index as usize;
        if index < TOTAL_DAYS {
            claimed[index] = row.claimed;
        }
    }

    let next_claimable = claimed.iter().position(|value| !*value);

    let now = chrono::Utc::now();
    let seconds_remaining = attendance_seconds_remaining(now);

    let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
    out.write_u8(EVENT_HUB_ATTENDANCE_SELECT); // outer selector: CUIAttendanceCheck
    out.write_i32(0); // error
    out.write_i32(1); // result: load/show panel
    out.write_i32(now.timestamp().clamp(0, i32::MAX as i64) as i32);
    out.write_i16(TOTAL_DAYS as i16);

    for day in 0..TOTAL_DAYS {
        let has_reward = rewards
            .iter()
            .find(|row| row.day_index as usize == day)
            .is_some_and(|row| row.item_id > 0);
        out.write_i32(if has_reward {
            attendance_daily_slot_key(day)
        } else {
            0
        });
        // Native CUIAttendanceCheck hides the reward group when state is 0.
        // State 3 keeps a future/locked reward visible but inactive.
        let state = if claimed[day] {
            1
        } else if Some(day) == next_claimable {
            3
        } else {
            3
        };
        out.write_u8(state);
    }

    let cumulative_items = cumulative
        .map(|row| [row.item1.unwrap_or(0), row.item2.unwrap_or(0), row.item3.unwrap_or(0)])
        .unwrap_or([0; 3]);
    out.write_i16(cumulative_items.len() as i16);
    for (index, item_id) in cumulative_items.into_iter().enumerate() {
        out.write_i32(if item_id > 0 {
            attendance_cumulative_slot_key(index)
        } else {
            0
        });
        let milestone = [7usize, 14, 21][index];
        let completed = claimed.iter().take(milestone).all(|value| *value);
        let state = if completed {
            1
        } else if claimed.iter().filter(|value| **value).count() + 1 == milestone {
            3
        } else {
            3
        };
        out.write_u8(state);
    }
    out.write_i32(seconds_remaining);

    session.send_packet(&out).await?;
    info!(
        "[{}] native attendance opened: rewards={} claimed={} granted_today={} seconds_remaining={}",
        session.addr(),
        rewards.len(),
        claimed.iter().filter(|value| **value).count(),
        granted,
        seconds_remaining
    );
    Ok(())
}

fn native_event_hub_packet(active: &[(u8, u8)]) -> Packet {
    let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
    out.write_u8(EVENT_HUB_SUB);
    out.write_i16(active.len().min(i16::MAX as usize) as i16);
    for &(event_id, enabled) in active.iter().take(i16::MAX as usize) {
        out.write_u8(event_id);
        out.write_u8(enabled);
    }
    out
}

async fn roulette_open(
    session: &mut ClientSession,
    repo: &NativeEventsRepository<'_>,
) -> anyhow::Result<()> {
    let free = repo.roulette_rewards(7).await.unwrap_or_default();
    let paid = repo.roulette_rewards(8).await.unwrap_or_default();
    if free.is_empty() || paid.is_empty() {
        let response = event_unavailable(Opcode::WizContinousPacketData as u8, 6);
        session.send_packet(&response).await?;
        return Ok(());
    }
    let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
    out.write_u8(6);
    out.write_i32(1);
    for reward in [&free[0], &paid[0]] {
        out.write_i32(reward.item_id);
        out.write_i32(reward.item_count as i32);
        out.write_i32(0);
    }
    session.send_packet(&out).await?;
    Ok(())
}

async fn roulette_spin(
    session: &mut ClientSession,
    repo: &NativeEventsRepository<'_>,
    name: &str,
    client_type: i32,
) -> anyhow::Result<()> {
    let kind = native_type(client_type);
    let rewards = repo.roulette_rewards(kind).await?;
    let total_weight: i32 = rewards.iter().map(|r| r.weight.max(1)).sum();
    if rewards.is_empty() || total_weight <= 0 { return Ok(()); }
    if repo.roulette_pending(name).await?.and_then(|p| p.pending_item_id).is_some() {
        let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
        out.write_u8(7); out.write_i32(client_type); out.write_i32(2);
        session.send_packet(&out).await?;
        return Ok(());
    }
    if client_type == ROULETTE_KC_TYPE
        && !knight_cash::cash_lose(session.world(), &session.pool().clone(), session.session_id(), ROULETTE_KC_COST)
    {
        let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
        out.write_u8(7); out.write_i32(client_type); out.write_i32(20);
        session.send_packet(&out).await?;
        return Ok(());
    }
    let mut roll = rand::thread_rng().gen_range(0..total_weight);
    let mut selected = &rewards[0];
    for reward in &rewards {
        roll -= reward.weight.max(1);
        if roll < 0 { selected = reward; break; }
    }
    if !repo.reserve_roulette_result(name, selected).await? { return Ok(()); }
    let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
    out.write_u8(7); out.write_i32(client_type); out.write_i32(1);
    session.send_packet(&out).await?;
    info!("native roulette reserved: character={name} type={kind} slot={}", selected.slot);
    Ok(())
}

async fn roulette_reveal(
    session: &mut ClientSession,
    repo: &NativeEventsRepository<'_>,
    name: &str,
    client_type: i32,
) -> anyhow::Result<()> {
    let pending = repo.roulette_pending(name).await?;
    let Some(p) = pending.filter(|p| p.pending_item_id.is_some()) else {
        let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
        out.write_u8(8); out.write_i32(client_type); out.write_i32(3);
        session.send_packet(&out).await?; return Ok(());
    };
    let item_id = p.pending_item_id.unwrap_or(0);
    let count = p.pending_item_count.unwrap_or(1).max(1) as u16;
    if !session.world().check_weight(session.session_id(), item_id as u32, count) {
        let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
        out.write_u8(8); out.write_i32(client_type); out.write_i32(21);
        session.send_packet(&out).await?; return Ok(());
    }
    let completed = repo.complete_roulette(name).await?;
    if completed.as_ref().and_then(|v| v.pending_item_id).is_none() { return Ok(()); }
    if !session.world().give_item(session.session_id(), item_id as u32, count) {
        warn!("native roulette give_item failed: {name} item={item_id}"); return Ok(());
    }
    send_roulette_result(session, client_type, &p).await
}

async fn send_roulette_result(
    session: &mut ClientSession,
    client_type: i32,
    p: &NativeRoulettePending,
) -> anyhow::Result<()> {
    let item = p.pending_item_id.unwrap_or(0);
    let count = p.pending_item_count.unwrap_or(1) as i32;
    let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
    out.write_u8(8); out.write_i32(client_type); out.write_i32(1);
    out.write_i32(p.pending_slot.unwrap_or(0) as i32 + 1);
    out.write_i32(item); out.write_i32(count);
    out.write_i32(item); out.write_i32(count); out.write_i32(0);
    out.write_i32(0); out.write_i32(0); out.write_i32(0);
    session.send_packet(&out).await?;
    Ok(())
}

async fn roulette_history(
    session: &mut ClientSession,
    repo: &NativeEventsRepository<'_>,
    name: &str,
    client_type: i32,
) -> anyhow::Result<()> {
    let rows = repo.roulette_history(name).await?;
    let mut out = Packet::new(Opcode::WizContinousPacketData as u8);
    out.write_u8(9); out.write_i32(client_type); out.write_i32(1);
    out.write_i32(rows.len().min(20) as i32);
    for row in rows.iter().take(20) {
        out.write_i32(row.item_id); out.write_i32(row.item_count as i32); out.write_i32(row.roulette_type as i32);
    }
    session.send_packet(&out).await?;
    Ok(())
}

pub async fn handle_jigsaw_coin(session: &mut ClientSession, pkt: Packet) -> anyhow::Result<()> {
    if session.state() != SessionState::InGame { return Ok(()); }
    let mut reader = PacketReader::new(&pkt.data);
    let sub = reader.read_u8().unwrap_or(0);
    let pool = session.pool().clone();
    let repo = NativeEventsRepository::new(&pool);
    let Some(name) = character_name(session) else { return Ok(()); };
    match sub {
        1 if repo.is_active("jigsaw").await.unwrap_or(false) => {
            jigsaw_open(session, &repo, &name).await
        }
        4 | 8 => {
            if repo.is_active("jigsaw").await.unwrap_or(false) {
                jigsaw_open(session, &repo, &name).await
            } else if repo.is_active("coin").await.unwrap_or(false) {
                coin_open(session, &repo, &name).await
            } else { Ok(()) }
        }
        5 if repo.is_active("jigsaw").await.unwrap_or(false) => {
            jigsaw_piece(session, &repo, &name, reader.read_u8().unwrap_or(255)).await
        }
        2 if repo.is_active("jigsaw").await.unwrap_or(false) => {
            jigsaw_claim(session, &repo, &name, reader.read_u8().unwrap_or(255)).await
        }
        2 if repo.is_active("coin").await.unwrap_or(false) => {
            coin_action(session, &repo, &name, reader.read_u8().unwrap_or(0)).await
        }
        _ => { debug!("[{}] native 0xCC ignored sub={sub}", session.addr()); Ok(()) }
    }
}

async fn jigsaw_open(session: &mut ClientSession, repo: &NativeEventsRepository<'_>, name: &str) -> anyhow::Result<()> {
    let state = repo.jigsaw_state(name).await?;
    let mut out = Packet::new(Opcode::WizEnchant as u8);
    out.write_u8(1); out.write_u8(1);
    out.write_u8(state.piece_counts.iter().map(|v| *v as i32).sum::<i32>().min(255) as u8);
    out.write_u8(8);
    for i in 0..8 { out.write_u8(state.piece_counts.get(i).copied().unwrap_or(0).clamp(0,255) as u8); }
    for i in 0..9 { out.write_u8(state.reward_claimed.get(i).copied().unwrap_or(false) as u8); }
    out.write_u8(0);
    session.send_packet(&out).await?; Ok(())
}

async fn jigsaw_piece(session: &mut ClientSession, repo: &NativeEventsRepository<'_>, name: &str, piece: u8) -> anyhow::Result<()> {
    let Some(_state) = repo.add_jigsaw_piece(name, piece as i16).await? else {
        let mut out = Packet::new(Opcode::WizEnchant as u8);
        out.write_u8(1); out.write_u8(4); out.write_u8(2);
        session.send_packet(&out).await?; return Ok(());
    };
    let mut out = Packet::new(Opcode::WizEnchant as u8);
    out.write_u8(1); out.write_u8(3); out.write_u8(piece); out.write_u8(0);
    session.send_packet(&out).await?; Ok(())
}

async fn jigsaw_claim(session: &mut ClientSession, repo: &NativeEventsRepository<'_>, name: &str, index: u8) -> anyhow::Result<()> {
    let grant = repo.claim_jigsaw(name, index as i16).await?;
    let success = if let Some(g) = grant {
        session.world().give_item(session.session_id(), g.item_id as u32, g.item_count.max(1) as u16)
    } else { false };
    let state = repo.jigsaw_state(name).await.unwrap_or(NativeJigsawState { piece_counts: vec![0;8], reward_claimed: vec![false;9] });
    let mut out = Packet::new(Opcode::WizEnchant as u8);
    out.write_u8(1); out.write_u8(2); out.write_u8(if success {1} else {4});
    if success { for i in 0..9 { out.write_u8(state.reward_claimed.get(i).copied().unwrap_or(false) as u8); } }
    session.send_packet(&out).await?; Ok(())
}

async fn coin_open(session: &mut ClientSession, repo: &NativeEventsRepository<'_>, name: &str) -> anyhow::Result<()> {
    let _state = repo.coin_state(name).await?;
    let mut out = Packet::new(Opcode::WizEnchant as u8);
    out.write_u8(2); out.write_u8(1); out.write_u8(1); out.write_u8(1);
    session.send_packet(&out).await?; Ok(())
}

async fn coin_action(session: &mut ClientSession, repo: &NativeEventsRepository<'_>, name: &str, action: u8) -> anyhow::Result<()> {
    let event_type = action.clamp(1, 5);
    let state = repo.add_coin_point(name).await?.unwrap_or(repo.coin_state(name).await?);
    if state.points >= 8 {
        if let Some(grant) = repo.claim_coin(name, (event_type - 1) as i16).await? {
            let _ = session.world().give_item(
                session.session_id(),
                grant.item_id as u32,
                grant.item_count.max(1) as u16,
            );
        }
    }
    let mut out = Packet::new(Opcode::WizEnchant as u8);
    out.write_u8(2); out.write_u8(3); out.write_u8(1);
    out.write_u8(event_type); out.write_u8(state.points.clamp(0,8) as u8);
    session.send_packet(&out).await?; Ok(())
}

pub async fn handle_marble(session: &mut ClientSession, pkt: Packet) -> anyhow::Result<()> {
    if session.state() != SessionState::InGame { return Ok(()); }
    let mut reader = PacketReader::new(&pkt.data);
    let sub = reader.read_u8().unwrap_or(0);
    let pool = session.pool().clone();
    let repo = NativeEventsRepository::new(&pool);
    if !repo.is_active("marble").await.unwrap_or(false) { return Ok(()); }
    let Some(name) = character_name(session) else { return Ok(()); };
    match sub {
        1 => marble_open(session, &repo, &name).await,
        2 => marble_roll(session, &repo, &name).await,
        3 | 4 | 5 => marble_open(session, &repo, &name).await,
        _ => Ok(()),
    }
}

async fn marble_open(session: &mut ClientSession, repo: &NativeEventsRepository<'_>, name: &str) -> anyhow::Result<()> {
    let state = repo.marble_state(name).await?;
    let mut out = Packet::new(Opcode::WizAbility as u8);
    out.write_u8(1); out.write_u8(1);
    out.write_u8(1); out.write_u8(state.position as u8); out.write_u8(0);
    out.write_u8(state.rolls_today.clamp(0,255) as u8); out.write_u8(state.laps.clamp(0,255) as u8);
    out.write_u64(0);
    session.send_packet(&out).await?; Ok(())
}

async fn marble_roll(session: &mut ClientSession, repo: &NativeEventsRepository<'_>, name: &str) -> anyhow::Result<()> {
    let die = rand::thread_rng().gen_range(1..=6) as i16;
    let Some((state,tile)) = repo.roll_marble(name, die).await? else {
        let mut out = Packet::new(Opcode::WizAbility as u8);
        out.write_u8(2); out.write_u8(3); session.send_packet(&out).await?; return Ok(());
    };
    if tile.item_id > 0 && tile.item_count > 0 {
        let _ = session.world().give_item(session.session_id(), tile.item_id as u32, tile.item_count as u16);
    }
    let mut ack = Packet::new(Opcode::WizAbility as u8);
    ack.write_u8(2); ack.write_u8(1); session.send_packet(&ack).await?;
    let mut out = Packet::new(Opcode::WizAbility as u8);
    out.write_u8(6); out.write_u8(1); out.write_u8(die as u8);
    out.write_u8(1); out.write_u8(state.position as u8); out.write_u8(tile.tile_type as u8);
    out.write_u8(state.rolls_today.clamp(0,255) as u8); out.write_u8(0); out.write_u8(0); out.write_u8(0); out.write_u8(0);
    session.send_packet(&out).await?;
    info!("native marble roll: character={name} die={die} position={}", state.position);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_roulette_types_map_to_tbl_groups() {
        assert_eq!(native_type(ROULETTE_FREE_TYPE), 7);
        assert_eq!(native_type(ROULETTE_KC_TYPE), 8);
    }

    #[test]
    fn unavailable_packet_uses_requested_sub() {
        let packet = event_unavailable(0x9C, 6);
        assert_eq!(packet.data, vec![6,0,0,0,0]);
    }

    #[test]
    fn event_hub_packet_matches_v2615_signed_f0_contract() {
        let packet = native_event_hub_packet(&[
            (EVENT_HUB_ATTENDANCE, 1),
            (EVENT_HUB_COIN, 1),
            (EVENT_HUB_ROULETTE, 1),
            (EVENT_HUB_JIGSAW, 1),
            (EVENT_HUB_MARBLE, 1),
        ]);
        assert_eq!(packet.opcode, Opcode::WizContinousPacketData as u8);
        assert_eq!(packet.data, vec![0xF0, 5, 0, 1, 1, 0, 1, 2, 1, 3, 1, 5, 1]);
    }

    #[test]
    fn attendance_wire_keys_match_v2615_calendar_lookup() {
        assert_eq!(attendance_daily_slot_key(0), 1);
        assert_eq!(attendance_daily_slot_key(24), 25);
        assert_eq!(attendance_cumulative_slot_key(0), 101);
        assert_eq!(attendance_cumulative_slot_key(2), 103);
    }
}
