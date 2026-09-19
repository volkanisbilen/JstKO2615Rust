local Ret = 0;
local NPC = 16047;

-- [Operator] Moira
-- Auto-generated from sniffer capture (dialog_builder v4)
-- 2 menus, 2 mapped, 0 inferred, 0 unknown

-- header=8917 flag=2
if (EVENT == 240) then
	SelectMsg(UID, 2, 16, 8917, NPC, 10, 3001);
end

-- header=4032 flag=2
if (EVENT == 241) then
	SelectMsg(UID, 2, 11, 4032, NPC, 10, 3001);
end

-- Close dialog
if (EVENT == 3001) then
	Ret = 1;
end

-- ═══════════════════════════════════════════════════════════════════
-- AUTO-GENERATED EVENT HANDLERS (ko-quest-gen)
-- ═══════════════════════════════════════════════════════════════════

-- [AUTO-GEN] quest=11 status=4 n_index=4041
if (EVENT == 280) then
	SelectMsg(UID, 2, 11, 228, NPC, 10, -1);
end

-- [AUTO-GEN] quest=13 status=4 n_index=4133
if (EVENT == 300) then
	SelectMsg(UID, 2, 13, 228, NPC, 10, -1);
end

-- [AUTO-GEN] quest=14 status=4 n_index=4427
if (EVENT == 400) then
	SelectMsg(UID, 2, 14, 4665, NPC, 10, -1);
end

-- [AUTO-GEN] quest=18 status=4 n_index=9772
if (EVENT == 450) then
	SelectMsg(UID, 2, 18, 4820, NPC, 10, -1);
end

-- [AUTO-GEN] quest=15 status=4 n_index=9857
if (EVENT == 500) then
	SelectMsg(UID, 2, 15, 228, NPC, 10, -1);
end

-- [AUTO-GEN] quest=16 status=4 n_index=1300
if (EVENT == 600) then
	SelectMsg(UID, 2, 16, 228, NPC, 10, -1);
end

-- [AUTO-GEN] quest=17 status=4 n_index=9773
if (EVENT == 700) then
	SelectMsg(UID, 2, 17, 9461, NPC, 10, -1);
end

-- [AUTO-GEN] quest=45 status=4 n_index=9774
if (EVENT == 800) then
	SelectMsg(UID, 2, 45, 12259, NPC, 10, -1);
end

-- [AUTO-GEN] quest=46 status=4 n_index=9775
if (EVENT == 2000) then
	SelectMsg(UID, 2, 46, 12390, NPC, 10, -1);
end

-- [AUTO-GEN] quest=1553 status=4 n_index=10540
if (EVENT == 2500) then
	SelectMsg(UID, 2, 1553, 228, NPC, 10, -1);
end

-- [AUTO-GEN] quest=1633 status=4 n_index=10839
if (EVENT == 3000) then
	SelectMsg(UID, 2, 1633, 228, NPC, 10, -1);
end

-- [AUTO-GEN] quest=1695 status=2 n_index=14237
if (EVENT == 4000) then
	QuestStatusCheck = GetQuestStatus(UID, 1695)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 6830)) then
			SaveEvent(UID, 14239);
		end
	end
end

