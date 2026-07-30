-- The verified 2614 DX11 client uses 2613 in the LoginServer and
-- GameServer version handshakes. Restore the value changed during
-- Manes Survival testing.
UPDATE server_settings
SET game_version = 2613
WHERE server_no = 1;
