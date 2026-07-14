-- Allow inventory slot index 96 for the 2614 DX11 client.
ALTER TABLE user_items DROP CONSTRAINT IF EXISTS chk_user_items_slot;
ALTER TABLE user_items ADD CONSTRAINT chk_user_items_slot CHECK (slot_index >= 0 AND slot_index < 97);
