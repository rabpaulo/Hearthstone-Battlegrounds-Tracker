// Battlegrounds combat simulator process for hsbg-overlay.
//
// Runs Firestone's simulator (@firestone-hs/simulate-bgs-battle) on combat
// snapshots sent by the overlay. JSON lines over stdio:
//
//   <- {"type":"ready","simulator":"1.1.771"}
//   -> {"id":1,"type":"simulate","sims":8000,"snapshot":{...}}
//   <- {"id":1,"type":"partial","result":{...}}   (zero or more)
//   <- {"id":1,"type":"result","result":{...}}  |  {"id":1,"type":"error","message":"..."}
//
// A new request cancels the one in flight: the overlay only cares about the
// combat on screen. The snapshot carries raw log entities (card id plus tags
// keyed by the name or number the log printed); turning them into the
// simulator's input happens here because it needs the simulator's GameTag
// and Race enums.
//
// Usage: node sim.mjs <cards_enUS.json>

import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { createInterface } from 'node:readline';

const require = createRequire(import.meta.url);
const sim = require('@firestone-hs/simulate-bgs-battle');
const { AllCardsService, GameTag, Race } = require('@firestone-hs/reference-data');

// The simulator logs to stdout; keep that channel for the protocol.
const send = (msg) => process.stdout.write(JSON.stringify(msg) + '\n');
console.log = console.info = console.warn = console.debug = (...a) => process.stderr.write(a.join(' ') + '\n');
console.time = console.timeEnd = () => {};

const cardsFile = process.argv[2];
if (!cardsFile) {
    process.stderr.write('usage: node sim.mjs <cards_enUS.json>\n');
    process.exit(2);
}
// Fields the simulator never reads; audio lists alone are most of the file.
const UNUSED_FIELDS = ['audio', 'audio2', 'flavor', 'artist', 'collectionText', 'howToEarn', 'howToEarnGolden'];
const cards = new AllCardsService();
cards.initializeCardsDbFromCards(
    JSON.parse(readFileSync(cardsFile, 'utf8'), (key, value) => (UNUSED_FIELDS.includes(key) ? undefined : value)),
);
sim.assignCards(cards);

// Placeholders shown before trinkets are picked; meaningless to the simulator.
const TRINKET_PLACEHOLDERS = new Set(['BG30_Trinket_1st', 'BG30_Trinket_2nd']);

// Player-entity counters the simulator reads from `globalInfo`.
const GLOBAL_INFO = {
    BloodGemAttackBonus: 'BACON_BLOODGEMBUFFATKVALUE',
    BloodGemHealthBonus: 'BACON_BLOODGEMBUFFHEALTHVALUE',
    ElementalAttackBuff: 'BACON_ELEMENTAL_BUFFATKVALUE',
    ElementalHealthBuff: 'BACON_ELEMENTAL_BUFFHEALTHVALUE',
    PirateAttackBuff: 'BACON_PIRATE_BUFFATKVALUE',
    PirateHealthBuff: 'BACON_PIRATE_BUFFHEALTHVALUE',
    TavernSpellAttackBuff: 'TAVERN_SPELL_ATTACK_INCREASE',
    TavernSpellHealthBuff: 'TAVERN_SPELL_HEALTH_INCREASE',
    TavernSpellsCastThisGame: 'TAVERN_SPELLS_PLAYED_THIS_GAME',
    SpellsCastThisGame: 'NUM_SPELLS_PLAYED_THIS_GAME',
    PiratesSummonedThisGame: 'BACON_PIRATES_SUMMONED_THIS_GAME',
    PiratesPlayedThisGame: 'BACON_PIRATES_PLAYED_THIS_GAME',
    BeastsSummonedThisGame: 'BACON_BEASTS_SUMMONED_THIS_GAME',
    GoldSpentThisGame: 'BACON_GOLD_SPENT_THIS_GAME',
    VolumizerAttackBuff: 'BACON_VOLUMIZER_ATTACK_BUFF',
    VolumizerHealthBuff: 'BACON_VOLUMIZER_HEALTH_BUFF',
    BattlecriesTriggeredThisGame: 'BATTLECRIES_TRIGGERED_THIS_GAME',
    DeathrattlesTriggeredThisGame: 'DEATHRATTLES_TRIGGERED_THIS_GAME',
    MagnetizedThisGame: 'BACON_NUM_MAGNETIZE_THIS_GAME',
    MrrgltonsPlayedThisGame: 'BACON_MRRGLTON_PLAYED_THIS_GAME',
    BackToBackCastThisGame: 'BACON_BACK_TO_BACK_CAST_THIS_GAME',
    CardsDiscardedThisGame: 'CARDS_DISCARDED_THIS_GAME',
    GoldenMinionsPlayedThisGame: 'BACON_GOLDEN_MINIONS_PLAYED_THIS_GAME',
    TastyLobstersBuff: 'BACON_TASTY_LOBSTER_BUFF',
};

/** Tag value by name, whether the log printed the name or the number. */
function tag(raw, name) {
    const byName = raw.tags[name];
    if (byName !== undefined) return byName;
    const num = GameTag[name];
    return num === undefined ? 0 : (raw.tags[String(num)] ?? 0);
}

function tagsByNumber(raw) {
    const out = {};
    for (const [k, v] of Object.entries(raw.tags)) {
        const n = /^\d+$/.test(k) ? Number(k) : GameTag[k];
        if (n !== undefined) out[n] = v;
    }
    return out;
}

function enchantment(raw) {
    return {
        cardId: raw.cardId,
        originEntityId: tag(raw, 'CREATOR'),
        tagScriptDataNum1: tag(raw, 'TAG_SCRIPT_DATA_NUM_1'),
        tagScriptDataNum2: tag(raw, 'TAG_SCRIPT_DATA_NUM_2'),
        timing: raw.id,
    };
}

function boardEntity(raw, friendly) {
    const health = tag(raw, 'HEALTH');
    const windfury = tag(raw, 'WINDFURY');
    return {
        entityId: raw.id,
        cardId: raw.cardId,
        attack: tag(raw, 'ATK'),
        health: health - tag(raw, 'DAMAGE'),
        maxHealth: health,
        taunt: tag(raw, 'TAUNT') === 1,
        divineShield: tag(raw, 'DIVINE_SHIELD') > 0,
        poisonous: tag(raw, 'POISONOUS') === 1,
        venomous: tag(raw, 'VENOMOUS') === 1,
        reborn: tag(raw, 'REBORN') === 1,
        stealth: tag(raw, 'STEALTH') === 1,
        windfury: windfury === 1 || windfury === 3 || tag(raw, 'MEGA_WINDFURY') === 1,
        scriptDataNum1: tag(raw, 'TAG_SCRIPT_DATA_NUM_1'),
        scriptDataNum2: tag(raw, 'TAG_SCRIPT_DATA_NUM_2'),
        scriptDataNum3: tag(raw, 'TAG_SCRIPT_DATA_NUM_3'),
        scriptDataNum4: tag(raw, 'TAG_SCRIPT_DATA_NUM_4'),
        scriptDataNum5: tag(raw, 'TAG_SCRIPT_DATA_NUM_5'),
        scriptDataNum6: tag(raw, 'TAG_SCRIPT_DATA_NUM_6'),
        enchantments: (raw.enchantments ?? []).map(enchantment),
        tags: tagsByNumber(raw),
        friendly,
    };
}

function boardInfo(side, friendly) {
    const hero = side.hero ?? { id: 0, cardId: '', tags: {} };
    const playerRaw = { tags: side.playerTags ?? {} };
    const globalInfo = {};
    for (const [field, name] of Object.entries(GLOBAL_INFO)) {
        const v = tag(playerRaw, name);
        if (v) globalInfo[field] = v;
    }
    const heroPowers = (side.heroPowers ?? []).map((hp) => ({
        cardId: hp.cardId,
        entityId: hp.id,
        used: tag(hp, 'EXHAUSTED') === 1 || tag(hp, 'BACON_HERO_POWER_ACTIVATED') === 1,
        activated: tag(hp, 'BACON_HERO_POWER_ACTIVATED') === 1,
        info: tag(hp, 'TAG_SCRIPT_DATA_NUM_1'),
        info2: tag(hp, 'TAG_SCRIPT_DATA_NUM_2'),
        info3: 0,
        info4: 0,
        info5: 0,
        info6: 0,
    }));
    const player = {
        cardId: hero.cardId,
        entityId: hero.id,
        hpLeft: tag(hero, 'HEALTH') - tag(hero, 'DAMAGE') + tag(hero, 'ARMOR'),
        tavernTier: tag(playerRaw, 'PLAYER_TECH_LEVEL') || 1,
        heroPowers,
        heroPowerId: heroPowers[0]?.cardId ?? null,
        heroPowerEntityId: heroPowers[0]?.entityId,
        heroPowerUsed: heroPowers[0]?.used ?? false,
        heroPowerInfo: heroPowers[0]?.info ?? 0,
        heroPowerInfo2: heroPowers[0]?.info2 ?? 0,
        friendly,
        questEntities: [],
        questRewards: (side.questRewards ?? []).map((q) => q.cardId),
        questRewardEntities: (side.questRewards ?? []).map((q) => ({
            cardId: q.cardId,
            entityId: q.id,
            scriptDataNum1: tag(q, 'TAG_SCRIPT_DATA_NUM_1'),
        })),
        hand: (side.hand ?? []).map((m) => boardEntity(m, friendly)),
        trinkets: (side.trinkets ?? [])
            .filter((t) => !TRINKET_PLACEHOLDERS.has(t.cardId))
            .map((t) => ({
                cardId: t.cardId,
                entityId: t.id,
                scriptDataNum1: tag(t, 'TAG_SCRIPT_DATA_NUM_1'),
                scriptDataNum2: tag(t, 'TAG_SCRIPT_DATA_NUM_2'),
                scriptDataNum6: tag(t, 'TAG_SCRIPT_DATA_NUM_6'),
            })),
        secrets: (side.secrets ?? []).map((s) => ({
            entityId: s.id,
            cardId: s.cardId,
            scriptDataNum1: tag(s, 'TAG_SCRIPT_DATA_NUM_1'),
            scriptDataNum2: tag(s, 'TAG_SCRIPT_DATA_NUM_2'),
        })),
        globalInfo,
        enchantments: (side.enchantments ?? []).map(enchantment),
    };
    return { player, board: (side.board ?? []).map((m) => boardEntity(m, friendly)) };
}

function battleInfo(snapshot, sims) {
    const validTribes = (snapshot.validTribes ?? []).map((t) => Race[t]).filter((r) => r !== undefined);
    return {
        playerBoard: boardInfo(snapshot.player, true),
        opponentBoard: boardInfo(snapshot.opponent, false),
        options: {
            numberOfSimulations: sims,
            maxAcceptableDuration: 6000,
            intermediateResults: 400,
            includeOutcomeSamples: false,
            skipInfoLogs: true,
        },
        gameState: {
            currentTurn: snapshot.turn,
            validTribes: validTribes.length ? validTribes : undefined,
            anomalies: snapshot.anomalies ?? [],
            numberOfPlayersAlive: snapshot.playersAlive,
        },
    };
}

function unknownCards(snapshot) {
    const ids = [];
    for (const side of [snapshot.player, snapshot.opponent]) {
        for (const e of [side.hero, ...(side.board ?? [])]) {
            if (e?.cardId && !cards.getCard(e.cardId)?.id) ids.push(e.cardId);
        }
    }
    return ids;
}

const summary = (r) => ({
    won: r.wonPercent ?? 0,
    tied: r.tiedPercent ?? 0,
    lost: r.lostPercent ?? 0,
    wonLethal: r.wonLethalPercent ?? 0,
    lostLethal: r.lostLethalPercent ?? 0,
    averageDamageWon: r.averageDamageWon ?? 0,
    averageDamageLost: r.averageDamageLost ?? 0,
    damageWonRange: r.damageWonRange ?? null,
    damageLostRange: r.damageLostRange ?? null,
    simulations: r.simulationsRun ?? 0,
});

let current = 0;
const yieldToEvents = () => new Promise((resolve) => setImmediate(resolve));

async function simulate(id, snapshot, sims) {
    current = id;
    const missing = unknownCards(snapshot);
    if (missing.length) {
        // An unknown card would be simulated as a blank 0/0 and skew the odds.
        send({ id, type: 'error', message: `unknown cards: ${missing.join(', ')}` });
        return;
    }
    const input = battleInfo(snapshot, sims);
    const cardsData = sim.prepareBattleCardsData(input, cards);
    const it = sim.simulateBattle(input, cards, cardsData);
    let step = it.next();
    while (!step.done) {
        if (step.value) send({ id, type: 'partial', result: summary(step.value) });
        await yieldToEvents();
        if (current !== id) return;
        step = it.next();
    }
    if (!step.value) {
        send({ id, type: 'error', message: 'simulator returned no result' });
        return;
    }
    send({ id, type: 'result', result: summary(step.value) });
}

createInterface({ input: process.stdin }).on('line', (line) => {
    let msg;
    try {
        msg = JSON.parse(line);
    } catch {
        return;
    }
    if (msg.type === 'simulate') {
        simulate(msg.id, msg.snapshot, msg.sims ?? 8000).catch((e) =>
            send({ id: msg.id, type: 'error', message: String(e?.stack ?? e) }),
        );
    }
}).on('close', () => process.exit(0));

send({ type: 'ready', simulator: sim.SIMULATOR_VERSION });
