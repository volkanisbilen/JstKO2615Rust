-- Restore the exact GameServer/LoginServer wire version used by the
-- stable-2614-first-ingame baseline. The executable expects 2599;
-- 2602 is the patch/files value and 2613 was introduced by mistake.
UPDATE server_settings
SET game_version = 2599
WHERE server_no = 1;
