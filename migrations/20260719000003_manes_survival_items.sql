-- Manes Survival event items for PostgreSQL.
-- This migration declares only the event-specific columns; the full item seed
-- supplies defaults for the remaining columns.

INSERT INTO item (
    num, extension, str_name, description, item_plus_id, item_alteration,
    item_icon_id1, item_icon_id2, kind, slot, race, countable, effect1,
    req_level, selling_group
) VALUES
    (978023000, 22, 'Exclusive Manes'' Survival HP Recovery Potion', '', '0', 0,
     97802300, 97802300, 97, 17, 20, 1, 491410, 0, 253),
    (978024000, 22, 'Exclusive Manes'' Survival MP Recovery Potion', '', '0', 0,
     97802400, 97802400, 97, 17, 20, 1, 491411, 0, 253),
    (978025000, 22, 'Exclusive Manes'' Survival Mid-Boss Summon Ticket', '', '0', 0,
     97802500, 97802500, 97, 15, 0, 0, 491412, 0, 0),
    (978026000, 22, 'Manes'' Orb', '', '0', 0,
     97802600, 97802600, 97, 15, 0, 1, 0, 0, 0)
ON CONFLICT (num) DO UPDATE SET
    extension = EXCLUDED.extension,
    str_name = EXCLUDED.str_name,
    description = EXCLUDED.description,
    item_plus_id = EXCLUDED.item_plus_id,
    item_alteration = EXCLUDED.item_alteration,
    item_icon_id1 = EXCLUDED.item_icon_id1,
    item_icon_id2 = EXCLUDED.item_icon_id2,
    kind = EXCLUDED.kind,
    slot = EXCLUDED.slot,
    race = EXCLUDED.race,
    countable = EXCLUDED.countable,
    effect1 = EXCLUDED.effect1,
    req_level = EXCLUDED.req_level,
    selling_group = EXCLUDED.selling_group;

DO $$
BEGIN
    IF (SELECT COUNT(*) FROM item WHERE num BETWEEN 978023000 AND 978026000) <> 4 THEN
        RAISE EXCEPTION 'Manes Survival item installation failed';
    END IF;
END $$;
