# Genie settings / monster drop policy, 2026-09-18

## Verified client protocol

`KnightOnLine_unpacked.exe`:

- `0xB19360`: sends `97 01 03`, then u16 populated skill count,
  count u32 skill/slot encodings, u16 42, and 42 option bytes.
- `0xB193D0` writes the count; `0xB193E4..0xB1940A` writes up to
  32 skill slots; `0xB1940C..0xB19419` writes the settings length.
- `0x7E4AC4` dispatch table, second entry `0x7E4A11`, routes subcommand
  2 to `0xB1A4B0`. This reads the same count, skill entries, two-byte
  settings length and settings data, then refreshes the Genie UI.
- Legal option payload length is 46 + 4 * skill_count (maximum 174).
  The old 100-byte storage assumption can truncate legal packets.

The server previously restored the session data at game-start phase 2,
but did not send the options response to the client there. Now it sends
`97 01 02` plus the saved valid blob after game initialization. SaveOptions
validates the complete payload and awaits its database write. Periodic,
logout and spirit/time saves no longer overwrite options with stale snapshots.
Invalid/legacy default blobs use the client's default settings structure.

## Drop policy

No destructive multiplication of database rows and no repeated migration:
the existing PostgreSQL rates remain the baseline. Each startup loads distinct
required item IDs from `quest_helper.n_exchange_index -> item_exchange` origin
items/counts for catalogue zones 1, 11, 12, 19. Zone 19 contains the shared
Eslant catalogue. Rewards are not used. Live query returned 228 distinct IDs.

- Monster items: baseline probability x1.15.
- Required collection items dropped in live zones 1, 11, 12: x1.60 instead.
- Resolve mixed item groups before choosing the multiplier, so unrelated
  items in a group are not given the collection bonus.
- Probabilities are rounded to the existing 1/10000 units, capped at 100%.
  Zero stays zero. Existing premium/event bonuses are still applied.
- GM `+drop` uses the same formula. Monster Stone and Manes special random
  drops also receive the general x1.15 multiplier; guaranteed rewards stay
  guaranteed. Item counts, gold quantities and non-monster NPC drops unchanged.

Backup before deployment: `/opt/ko-backups/20260918-genie-drops/`, including
full PostgreSQL dump, previous live binary and changed source files.

Player verification: choose Genie skills and non-default settings, close the
settings dialog to save, relog and reopen it. Server INFO logs now record both
SaveOptions and LoadOptions byte lengths. Client gameplay verification remains
the player's test; protocol/unit/build checks do not substitute for it.
