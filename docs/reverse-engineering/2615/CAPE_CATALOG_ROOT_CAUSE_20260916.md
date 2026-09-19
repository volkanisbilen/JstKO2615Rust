# Cape catalog failure: 2026-09-16

The GameServer 0x2B response prefix is a catalog selector, not a success byte.

## Evidence from KnightOnLine_unpacked.exe

- 0x007AF785–0x007AF7AA reads the prefix into global 0x011058FC + 0x818.
- 0x007AF831–0x007AF87E: zero selects Data\\Cloak.tbl.
- 0x007AF880–0x007AF8CF: nonzero selects Data\\Cloak_PVP.tbl.
- 0x007AF92E loads the selected table. Version/key decoding follows at 0x007AF992.
- Table loader sub_52A840 requires aligned record size 0x4C (check at 0x0052AACA).
- Actual client Cloak.tbl: 10 columns, 500 rows, aligned size 0x4C.
- Actual client Cloak_pvp.tbl: 7 columns, 224 rows, aligned size 0x30: rejected.
- Shop sub_BB5350 uses global table 0x0110583C (0x00BB53D6).
- Character cape path 0x008EE752 calls sub_4E83C0, which uses that same table.

Decoded client log.klg confirms: `[22:53:40] N3TableBase - incorrect table (Data\\Cloak_PVP.tbl)`.
The .klg records are length-prefixed encrypted text: u32 length, reset key 0x816;
plaintext = ciphertext XOR (key >> 8); key = ((ciphertext + key) * 0x6081 + 0x1608) & 0xffff.

## Fix

Use prefix zero in prefixed version modes, including live DB-version mode 99.
Preserve version values, AES key layout, trailers, MyInfo layout, launcher, database and client assets.
Regression tests verify exact payloads for every prefixed mode. Legacy no-prefix modes 4/5 are unchanged.
Visual confirmation requires a fresh client connection after deployment; no quest/clan reset is needed.
