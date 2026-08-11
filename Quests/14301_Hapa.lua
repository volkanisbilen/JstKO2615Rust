local Ret = 0;
local NPC = 14301;

-- [Blacksmith] Heppa
-- Auto-generated from sniffer capture (dialog_builder v4)
-- 1 menus, 0 mapped, 1 inferred, 0 unknown

-- header=615 flag=1
if (EVENT == 240) then
	SelectMsg(UID, 1, 94, 615, NPC, 14, 3001);
end

-- Close dialog
if (EVENT == 3001) then
	Ret = 1;
end

-- ═══════════════════════════════════════════════════════════════════
-- AUTO-GENERATED EVENT HANDLERS (ko-quest-gen)
-- ═══════════════════════════════════════════════════════════════════

-- [AUTO-GEN] quest=491 status=2 n_index=2406
if (EVENT == 241) then
	QuestStatusCheck = GetQuestStatus(UID, 491)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		SaveEvent(UID, 2408);
	end
end

-- [AUTO-GEN] quest=81 status=255 n_index=358
if (EVENT == 301) then
	SaveEvent(UID, 360);
end

-- [AUTO-GEN] quest=81 status=0 n_index=360
if (EVENT == 302) then
	SelectMsg(UID, 4, 81, 602, NPC, 31, 303, 23, -1);
end

-- [AUTO-GEN] quest=81 status=0 n_index=360
if (EVENT == 303) then
	SaveEvent(UID, 361);
end

-- [AUTO-GEN] quest=81 status=1 n_index=361
if (EVENT == 306) then
	QuestStatusCheck = GetQuestStatus(UID, 81)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 56)) then
			SaveEvent(UID, 362);
		end
	end
end

-- [AUTO-GEN] quest=81 status=1 n_index=361
if (EVENT == 308) then
	ItemA = HowmuchItem(UID, 810418000);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 81, 602, NPC, 18, 309);
	else
		SelectMsg(UID, 4, 81, 602, NPC, 41, 306, 27, -1);
	end
end

-- [AUTO-GEN] quest=81 status=1 n_index=361
if (EVENT == 309) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=89 status=255 n_index=380
if (EVENT == 313) then
	SaveEvent(UID, 382);
end

-- [AUTO-GEN] quest=89 status=0 n_index=382
if (EVENT == 315) then
	SelectMsg(UID, 4, 89, 613, NPC, 32, 316, 23, -1);
end

-- [AUTO-GEN] quest=89 status=0 n_index=382
if (EVENT == 316) then
	SaveEvent(UID, 383);
end

-- [AUTO-GEN] quest=89 status=1 n_index=383
if (EVENT == 319) then
	QuestStatusCheck = GetQuestStatus(UID, 89)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 60)) then
			SaveEvent(UID, 384);
		end
	end
end

-- [AUTO-GEN] quest=89 status=1 n_index=383
if (EVENT == 321) then
	ItemA = HowmuchItem(UID, 910017000);
	if (ItemA < 1000) then
		SelectMsg(UID, 2, 89, 613, NPC, 18, 322);
	else
		SelectMsg(UID, 4, 89, 613, NPC, 41, 319, 27, -1);
	end
end

-- [AUTO-GEN] quest=89 status=1 n_index=383
if (EVENT == 322) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=94 status=255 n_index=402
if (EVENT == 327) then
	SaveEvent(UID, 404);
end

-- [AUTO-GEN] quest=94 status=0 n_index=404
if (EVENT == 329) then
	SelectMsg(UID, 4, 94, 622, NPC, 33, 330, 23, -1);
end

-- [AUTO-GEN] quest=94 status=0 n_index=404
if (EVENT == 330) then
	SaveEvent(UID, 405);
end

-- [AUTO-GEN] quest=94 status=1 n_index=405
if (EVENT == 333) then
	QuestStatusCheck = GetQuestStatus(UID, 94)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 64)) then
			SaveEvent(UID, 406);
		end
	end
end

-- [AUTO-GEN] quest=94 status=1 n_index=405
if (EVENT == 335) then
	ItemA = HowmuchItem(UID, 810418000);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 94, 622, NPC, 18, 336);
	else
		SelectMsg(UID, 4, 94, 622, NPC, 41, 333, 27, -1);
	end
end

-- [AUTO-GEN] quest=94 status=1 n_index=405
if (EVENT == 336) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=192 status=255 n_index=409
if (EVENT == 338) then
	SaveEvent(UID, 411);
end

-- [AUTO-GEN] quest=192 status=4 n_index=411
if (EVENT == 340) then
	SelectMsg(UID, 2, 192, 651, NPC, 10, -1);
end

-- [AUTO-GEN] quest=154 status=255 n_index=624
if (EVENT == 400) then
	SaveEvent(UID, 626);
end

-- [AUTO-GEN] quest=154 status=4 n_index=626
if (EVENT == 402) then
	SelectMsg(UID, 2, 154, 1223, NPC, 10, -1);
end

-- [AUTO-GEN] quest=205 status=255 n_index=646
if (EVENT == 450) then
	SaveEvent(UID, 648);
end

-- [AUTO-GEN] quest=205 status=4 n_index=648
if (EVENT == 451) then
	SelectMsg(UID, 2, 205, 1292, NPC, 10, -1);
end

-- [AUTO-GEN] quest=93 status=255 n_index=652
if (EVENT == 470) then
	SaveEvent(UID, 654);
end

-- [AUTO-GEN] quest=93 status=0 n_index=654
if (EVENT == 471) then
	SelectMsg(UID, 4, 93, 1303, NPC, 50, 472, 23, -1);
end

-- [AUTO-GEN] quest=93 status=0 n_index=654
if (EVENT == 472) then
	SaveEvent(UID, 655);
end

-- [AUTO-GEN] quest=93 status=1 n_index=655
if (EVENT == 474) then
	ItemA = HowmuchItem(UID, 810418000);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 93, 1303, NPC, 18, 475);
	else
		SelectMsg(UID, 4, 93, 1303, NPC, 41, 483, 27, -1);
	end
end

-- [AUTO-GEN] quest=93 status=1 n_index=655
if (EVENT == 475) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=93 status=1 n_index=655
if (EVENT == 483) then
	QuestStatusCheck = GetQuestStatus(UID, 93)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 108)) then
			SaveEvent(UID, 656);
		end
	end
end

-- [AUTO-GEN] quest=69 status=255 n_index=10217
if (EVENT == 554) then
	SaveEvent(UID, 10219);
end

-- [AUTO-GEN] quest=69 status=0 n_index=10219
if (EVENT == 555) then
	SelectMsg(UID, 4, 69, 6533, NPC, 995, 556, 23, -1);
end

-- [AUTO-GEN] quest=69 status=0 n_index=10219
if (EVENT == 556) then
	SaveEvent(UID, 10220);
end

-- [AUTO-GEN] quest=69 status=1 n_index=10220
if (EVENT == 559) then
	QuestStatusCheck = GetQuestStatus(UID, 69)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 208)) then
			SaveEvent(UID, 10221);
		end
	end
end

-- [AUTO-GEN] quest=69 status=2 n_index=10221
if (EVENT == 563) then
	QuestStatusCheck = GetQuestStatus(UID, 69)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 208)) then
			SaveEvent(UID, 10223);
		end
	end
end

-- [AUTO-GEN] quest=69 status=1 n_index=10220
if (EVENT == 565) then
	ItemA = HowmuchItem(UID, 110110041);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 69, 6533, NPC, 18, 566);
	else
		SelectMsg(UID, 4, 69, 6533, NPC, 41, 559, 27, -1);
	end
end

-- [AUTO-GEN] quest=69 status=1 n_index=10220
if (EVENT == 566) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=491 status=0 n_index=2404
if (EVENT == 1555) then
	SelectMsg(UID, 4, 491, 0, NPC, 2030, 1556, 23, -1);
end

-- [AUTO-GEN] quest=491 status=0 n_index=2404
if (EVENT == 1556) then
	SaveEvent(UID, 2405);
end

-- [AUTO-GEN] quest=491 status=1 n_index=2405
if (EVENT == 1565) then
	SelectMsg(UID, 2, 491, 0, NPC, 10, -1);
end

-- [AUTO-GEN] quest=491 status=1 n_index=2405
if (EVENT == 1566) then
	QuestStatusCheck = GetQuestStatus(UID, 491)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		SaveEvent(UID, 2406);
	end
end

-- [AUTO-GEN] quest=491 status=1 n_index=2405
if (EVENT == 1567) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=514 status=0 n_index=2603
if (EVENT == 1655) then
	SelectMsg(UID, 4, 514, 0, NPC, 2031, 1656, 23, -1);
end

-- [AUTO-GEN] quest=514 status=0 n_index=2603
if (EVENT == 1656) then
	SaveEvent(UID, 2604);
end

-- [AUTO-GEN] quest=514 status=1 n_index=2604
if (EVENT == 1665) then
	SelectMsg(UID, 2, 514, 0, NPC, 10, -1);
end

-- [AUTO-GEN] quest=514 status=1 n_index=2604
if (EVENT == 1666) then
	QuestStatusCheck = GetQuestStatus(UID, 514)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		SaveEvent(UID, 2605);
	end
end

-- [AUTO-GEN] quest=514 status=1 n_index=2604
if (EVENT == 1667) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=515 status=0 n_index=2608
if (EVENT == 1755) then
	SelectMsg(UID, 4, 515, 0, NPC, 2032, 1756, 23, -1);
end

-- [AUTO-GEN] quest=515 status=0 n_index=2608
if (EVENT == 1756) then
	SaveEvent(UID, 2609);
end

-- [AUTO-GEN] quest=515 status=1 n_index=2609
if (EVENT == 1765) then
	SelectMsg(UID, 2, 515, 0, NPC, 10, -1);
end

-- [AUTO-GEN] quest=515 status=1 n_index=2609
if (EVENT == 1766) then
	QuestStatusCheck = GetQuestStatus(UID, 515)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		SaveEvent(UID, 2610);
	end
end

-- [AUTO-GEN] quest=515 status=1 n_index=2609
if (EVENT == 1767) then
	ShowMap(UID, 21);
end

-- [AUTO-GEN] quest=68 status=255 n_index=658
if (EVENT == 4009) then
	SaveEvent(UID, 660);
end

-- [AUTO-GEN] quest=68 status=0 n_index=660
if (EVENT == 4011) then
	SelectMsg(UID, 4, 68, 4017, NPC, 300, 4012, 23, -1);
end

-- [AUTO-GEN] quest=68 status=0 n_index=660
if (EVENT == 4012) then
	SaveEvent(UID, 661);
end

-- [AUTO-GEN] quest=68 status=1 n_index=661
if (EVENT == 4016) then
	QuestStatusCheck = GetQuestStatus(UID, 68)
	if(QuestStatusCheck == 2) then
		SelectMsg(UID, 2, -1, 8779, NPC, 10, -1);
	else
		if (RunQuestExchange(UID, 405)) then
			SaveEvent(UID, 662);
		end
	end
end

-- [AUTO-GEN] quest=68 status=1 n_index=661
if (EVENT == 4020) then
	ItemA = HowmuchItem(UID, 110110002);
	if (ItemA < 0) then
		SelectMsg(UID, 2, 68, 4017, NPC, 18, 4021);
	else
		SelectMsg(UID, 4, 68, 4017, NPC, 41, 4016, 27, -1);
	end
end

-- [AUTO-GEN] quest=68 status=1 n_index=661
if (EVENT == 4021) then
	ShowMap(UID, 21);
end

