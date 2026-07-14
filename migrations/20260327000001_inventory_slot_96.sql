-- Expand user_items slot_index constraint to allow slot indices 0 through 96 (v2600: 3 magic bags + knight royale).
-- Sniffer verified: 2614 DX11 live test verified slot index 96 requires an upper bound of 97.
ALTER TABLE user_items DROP CONSTRAINT IF EXISTS chk_user_items_slot;
ALTER TABLE user_items ADD CONSTRAINT chk_user_items_slot CHECK (slot_index >= 0 AND slot_index < 97);
