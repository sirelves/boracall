-- BoraCall — remove o modelo de sala avulsa.
--
-- Substituído por servidor → canais (migrations 0002 e 0003). Nada no código
-- aponta mais pra cá: os handlers de /api/rooms e o WebSocket por sala saíram
-- junto com esta migration.
--
-- call_events cai junto: referencia rooms(id), nunca recebeu um insert desde
-- que foi criada (era a issue #11) e o modelo de evento que faz sentido agora
-- é por canal, não por sala. Volta quando houver uso real pra ele.

DROP TABLE IF EXISTS call_events;
DROP TABLE IF EXISTS memberships;
DROP TABLE IF EXISTS rooms;
