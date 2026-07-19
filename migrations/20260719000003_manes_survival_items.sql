-- Manes Survival event items for PostgreSQL.
-- Idempotent: safe to run repeatedly from DBeaver or psql.

INSERT INTO item (
    num, extension, str_name, description, item_plus_id, item_alteration,
    item_icon_id1, item_icon_id2, kind, slot, race, class,
    weight, duration, buy_price, sell_price, ac, countable,
    effect1, effect2, req_level, req_level_max, selling_group, item_type
) VALUES
(978023000,22,'Exclusive Manes'' Survival HP Recovery Potion','','0',0,
 97802300,97802300,97,17,20,0,0,1,1,0,0,1,491410,0,0,100,253,0),
(978024000,22,'Exclusive Manes'' Survival MP Recovery Potion','','0',0,
 97802400,97802400,97,17,20,0,0,1,1,0,0,1,491411,0,0,100,253,0),
(978025000,22,'Exclusive Manes'' Survival Mid-Boss Summon Ticket','','0',0,
 97802500,97802500,97,15,0,0,1,3,2500,0,0,0,491412,0,0,100,0,0),
(978026000,22,'Manes'' Orb','','0',0,
 97802600,97802600,97,15,0,0,1,1,50,0,0,1,0,0,0,100,0,0)
ON CONFLICT (num) DO UPDATE SET
    extension       = EXCLUDED.extension,
    str_name        = EXCLUDED.str_name,
    description     = EXCLUDED.description,
    item_plus_id    = EXCLUDED.item_plus_id,
    item_alteration = EXCLUDED.item_alteration,
    item_icon_id1   = EXCLUDED.item_icon_id1,
    item_icon_id2   = EXCLUDED.item_icon_id2,
    kind            = EXCLUDED.kind,
    slot            = EXCLUDED.slot,
    race            = EXCLUDED.race,
    class           = EXCLUDED.class,
    weight          = EXCLUDED.weight,
    duration        = EXCLUDED.duration,
    buy_price       = EXCLUDED.buy_price,
    sell_price      = EXCLUDED.sell_price,
    ac              = EXCLUDED.ac,
    countable       = EXCLUDED.countable,
    effect1         = EXCLUDED.effect1,
    effect2         = EXCLUDED.effect2,
    req_level       = EXCLUDED.req_level,
    req_level_max   = EXCLUDED.req_level_max,
    selling_group   = EXCLUDED.selling_group,
    item_type       = EXCLUDED.item_type;

DO $$
BEGIN
    IF (SELECT COUNT(*) FROM item WHERE num BETWEEN 978023000 AND 978026000) <> 4 THEN
        RAISE EXCEPTION 'Manes Survival item installation failed';
    END IF;
END $$;
