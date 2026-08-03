-- v2615 Event Post-Up board, opened only by quest-helper 13685's NPC proto 24407.
-- One row is one successful reward delivery; ISO-week enforcement is handled
-- transactionally by NativeEventsRepository::reserve_board_claim.

CREATE TABLE IF NOT EXISTS native_board_claim (
    id BIGSERIAL PRIMARY KEY,
    character_name VARCHAR(21) NOT NULL,
    item_id INTEGER NOT NULL DEFAULT 811084000,
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_native_board_claim_character_week
    ON native_board_claim (character_name, claimed_at DESC);

COMMENT ON TABLE native_board_claim IS
    'Successful quest-helper 13685 / NPC proto 24407 I Love Knight Online board rewards; maximum 3 per ISO week';
