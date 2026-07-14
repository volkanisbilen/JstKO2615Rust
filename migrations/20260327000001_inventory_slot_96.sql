-- Expand user_items slot_index constraint from 77 to 96 (v2600: 3 magic bags + knight royale).
-- Sniffer verified: original server sends 96 items in MyInfo.
ALTER TABLE user_items DROP CONSTRAINT IF EXISTS chk_user_items_slot;
ALTER TABLE user_items ADD CONSTRAINT chk_user_items_slot CHECK (slot_index >= 0 AND slot_index < 96);
