local Ret = 0;
local NPC = 19002;

-- [Entrep Trader] Berret
-- Auto-generated from sniffer capture (dialog_builder v4)
-- 1 menus, 0 mapped, 1 inferred, 0 unknown [actions=SHOP]

-- header=4947 flag=1
if (EVENT == 165) then
	SelectMsg(UID, 1, 95, 4947, NPC, 28, 5000);
end

-- ═══ Action handlers (sniffer-verified) ═══

-- SHOP action (btn_text=28)
if (EVENT == 5000) then
	SelectMsg(UID, 21, -1, -1, NPC, -1, -1); -- selling_group=0, fallback to flag 21
end

-- Close dialog
if (EVENT == 3001) then
	Ret = 1;
end

-- ═══════════════════════════════════════════════════════════════════
-- AUTO-GENERATED EVENT HANDLERS (ko-quest-gen)
-- ═══════════════════════════════════════════════════════════════════

-- [AUTO-GEN] quest=117 status=255 n_index=9957
if (EVENT == 100) then
	SearchQuest(UID, 19002);
end

-- [AUTO-GEN] quest=117 status=0 n_index=9958
if (EVENT == 102) then
	SelectMsg(UID, 4, 117, 4943, NPC, 948, 103, 23, -1);
end

-- [AUTO-GEN] quest=117 status=0 n_index=9958
if (EVENT == 103) then
	SaveEvent(UID, 9959);
end

-- [AUTO-GEN] quest=117 status=1 n_index=9959
if (EVENT == 105) then
	QuestStatusCheck = GetQuestStatus(UID, 117)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 537)) then
			SaveEvent(UID, 9960);
		end
	end
end

-- [AUTO-GEN] quest=117 status=1 n_index=9959
if (EVENT == 106) then
	ItemA = HowmuchItem(UID, 810418000);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 117, 4943, NPC, 18, 107);
	else
		SelectMsg(UID, 4, 117, 4943, NPC, 41, 105, 27, -1);
	end
end

-- [AUTO-GEN] quest=117 status=1 n_index=9959
if (EVENT == 107) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=95 status=2 n_index=533
if (EVENT == 190) then
	SearchQuest(UID, 19002);
end

-- [AUTO-GEN] quest=119 status=255 n_index=9969
if (EVENT == 200) then
	SaveEvent(UID, 9970);
end

-- [AUTO-GEN] quest=119 status=0 n_index=9970
if (EVENT == 202) then
	SelectMsg(UID, 4, 119, 4955, NPC, 949, 203, 23, -1);
end

-- [AUTO-GEN] quest=119 status=0 n_index=9970
if (EVENT == 203) then
	SaveEvent(UID, 9971);
end

-- [AUTO-GEN] quest=119 status=1 n_index=9971
if (EVENT == 205) then
	QuestStatusCheck = GetQuestStatus(UID, 119)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 538)) then
			SaveEvent(UID, 9972);
		end
	end
end

-- [AUTO-GEN] quest=119 status=1 n_index=9971
if (EVENT == 206) then
	ItemA = HowmuchItem(UID, 810418000);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 119, 4955, NPC, 18, 207);
	else
		SelectMsg(UID, 4, 119, 4955, NPC, 41, 205, 27, -1);
	end
end

-- [AUTO-GEN] quest=119 status=1 n_index=9971
if (EVENT == 207) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=121 status=255 n_index=9981
if (EVENT == 210) then
	SaveEvent(UID, 9982);
end

-- [AUTO-GEN] quest=121 status=0 n_index=9982
if (EVENT == 212) then
	SelectMsg(UID, 4, 121, 571, NPC, 950, 213, 23, -1);
end

-- [AUTO-GEN] quest=121 status=0 n_index=9982
if (EVENT == 213) then
	SaveEvent(UID, 9983);
end

-- [AUTO-GEN] quest=121 status=1 n_index=9983
if (EVENT == 215) then
	QuestStatusCheck = GetQuestStatus(UID, 121)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 539)) then
			SaveEvent(UID, 9984);
		end
	end
end

-- [AUTO-GEN] quest=121 status=1 n_index=9983
if (EVENT == 216) then
	ItemA = HowmuchItem(UID, 810418000);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 121, 571, NPC, 18, 217);
	else
		SelectMsg(UID, 4, 121, 571, NPC, 41, 215, 27, -1);
	end
end

-- [AUTO-GEN] quest=121 status=1 n_index=9983
if (EVENT == 217) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=123 status=255 n_index=9993
if (EVENT == 220) then
	SaveEvent(UID, 9994);
end

-- [AUTO-GEN] quest=123 status=0 n_index=9994
if (EVENT == 222) then
	SelectMsg(UID, 4, 123, 4979, NPC, 951, 223, 23, -1);
end

-- [AUTO-GEN] quest=123 status=0 n_index=9994
if (EVENT == 223) then
	SaveEvent(UID, 9995);
end

-- [AUTO-GEN] quest=123 status=1 n_index=9995
if (EVENT == 225) then
	QuestStatusCheck = GetQuestStatus(UID, 123)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 540)) then
			SaveEvent(UID, 9996);
		end
	end
end

-- [AUTO-GEN] quest=123 status=1 n_index=9995
if (EVENT == 226) then
	ItemA = HowmuchItem(UID, 810418000);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 123, 4979, NPC, 18, 227);
	else
		SelectMsg(UID, 4, 123, 4979, NPC, 41, 225, 27, -1);
	end
end

-- [AUTO-GEN] quest=123 status=1 n_index=9995
if (EVENT == 227) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=95 status=255 n_index=530
if (EVENT == 320) then
	SaveEvent(UID, 531);
end

-- [AUTO-GEN] quest=95 status=0 n_index=531
if (EVENT == 322) then
	SelectMsg(UID, 4, 95, 663, NPC, 39, 323, 23, -1);
end

-- [AUTO-GEN] quest=95 status=0 n_index=531
if (EVENT == 323) then
	SaveEvent(UID, 532);
end

-- [AUTO-GEN] quest=95 status=1 n_index=532
if (EVENT == 325) then
	QuestStatusCheck = GetQuestStatus(UID, 95)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 85)) then
			SaveEvent(UID, 533);
		end
	end
end

-- [AUTO-GEN] quest=95 status=1 n_index=532
if (EVENT == 326) then
	ItemA = HowmuchItem(UID, 810418000);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 95, 663, NPC, 18, 327);
	else
		SelectMsg(UID, 4, 95, 663, NPC, 41, 325, 27, -1);
	end
end

-- [AUTO-GEN] quest=95 status=1 n_index=532
if (EVENT == 327) then
	ShowMap(UID, 21);
end

