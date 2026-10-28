//! Tag names and the enum values Hearthstone prints by name.

pub const ZONE: &str = "ZONE";
pub const ZONE_POSITION: &str = "ZONE_POSITION";
pub const CONTROLLER: &str = "CONTROLLER";
pub const CARDTYPE: &str = "CARDTYPE";
pub const ATTACHED: &str = "ATTACHED";
pub const STEP: &str = "STEP";
pub const TURN: &str = "TURN";
pub const HEALTH: &str = "HEALTH";
pub const DAMAGE: &str = "DAMAGE";
pub const ARMOR: &str = "ARMOR";
pub const ATK: &str = "ATK";
pub const PLAYER_ID: &str = "PLAYER_ID";
pub const HERO_ENTITY: &str = "HERO_ENTITY";
pub const PLAYER_TECH_LEVEL: &str = "PLAYER_TECH_LEVEL";
pub const PLAYER_LEADERBOARD_PLACE: &str = "PLAYER_LEADERBOARD_PLACE";
pub const BACON_DUMMY_PLAYER: &str = "BACON_DUMMY_PLAYER";
pub const BACON_CURRENT_COMBAT_PLAYER_ID: &str = "BACON_CURRENT_COMBAT_PLAYER_ID";
pub const BACON_COMBAT_PHASE_HERO: &str = "BACON_COMBAT_PHASE_HERO";
pub const TAUNT: &str = "TAUNT";
pub const DIVINE_SHIELD: &str = "DIVINE_SHIELD";
pub const POISONOUS: &str = "POISONOUS";
pub const VENOMOUS: &str = "VENOMOUS";
pub const REBORN: &str = "REBORN";
pub const WINDFURY: &str = "WINDFURY";
pub const STEALTH: &str = "STEALTH";
pub const PREMIUM: &str = "PREMIUM";
pub const BACON_BLOODGEMBUFFATKVALUE: &str = "BACON_BLOODGEMBUFFATKVALUE";
pub const BACON_BLOODGEMBUFFHEALTHVALUE: &str = "BACON_BLOODGEMBUFFHEALTHVALUE";
pub const BACON_FREE_REFRESH_COUNT: &str = "BACON_FREE_REFRESH_COUNT";
pub const TAVERN_SPELL_ATTACK_INCREASE: &str = "TAVERN_SPELL_ATTACK_INCREASE";
pub const TAVERN_SPELL_HEALTH_INCREASE: &str = "TAVERN_SPELL_HEALTH_INCREASE";
pub const BACON_PLAYER_EXTRA_GOLD_NEXT_TURN: &str = "BACON_PLAYER_EXTRA_GOLD_NEXT_TURN";
pub const BACON_COMBAT_DAMAGE_CAP: &str = "BACON_COMBAT_DAMAGE_CAP";
pub const BACON_COMBAT_DAMAGE_CAP_ENABLED: &str = "BACON_COMBAT_DAMAGE_CAP_ENABLED";
pub const TAG_SCRIPT_DATA_NUM_1: &str = "TAG_SCRIPT_DATA_NUM_1";
pub const TAG_SCRIPT_DATA_NUM_2: &str = "TAG_SCRIPT_DATA_NUM_2";

pub mod zone {
    pub const PLAY: i32 = 1;
    pub const HAND: i32 = 3;
    pub const REMOVEDFROMGAME: i32 = 5;
    pub const SECRET: i32 = 7;
}

pub mod card_type {
    pub const HERO: i32 = 3;
    pub const MINION: i32 = 4;
    pub const ENCHANTMENT: i32 = 6;
    pub const HERO_POWER: i32 = 10;
    pub const BATTLEGROUND_QUEST_REWARD: i32 = 40;
    pub const BATTLEGROUND_ANOMALY: i32 = 43;
    pub const BATTLEGROUND_TRINKET: i32 = 44;
}

pub mod step {
    pub const FINAL_GAMEOVER: i32 = 15;
}

/// Converts a printed tag value to its number. Numeric values parse as-is;
/// named values are resolved for the enum tags the tracker reads. Named values
/// of other tags return `None` and are not stored.
pub fn parse_value(tag: &str, value: &str) -> Option<i32> {
    if let Ok(v) = value.parse() {
        return Some(v);
    }
    let table: &[(&str, i32)] = match tag {
        "ZONE" => &[
            ("INVALID", 0),
            ("PLAY", 1),
            ("DECK", 2),
            ("HAND", 3),
            ("GRAVEYARD", 4),
            ("REMOVEDFROMGAME", 5),
            ("SETASIDE", 6),
            ("SECRET", 7),
            ("LETTUCE_ABILITY", 8),
            ("COSMETIC", 9),
        ],
        "CARDTYPE" => &[
            ("INVALID", 0),
            ("GAME", 1),
            ("PLAYER", 2),
            ("HERO", 3),
            ("MINION", 4),
            ("SPELL", 5),
            ("ENCHANTMENT", 6),
            ("WEAPON", 7),
            ("ITEM", 8),
            ("TOKEN", 9),
            ("HERO_POWER", 10),
            ("BLANK", 11),
            ("GAME_MODE_BUTTON", 12),
            ("MOVE_MINION_HOVER_TARGET", 22),
            ("LETTUCE_ABILITY", 23),
            ("BATTLEGROUND_HERO_BUDDY", 24),
            ("LOCATION", 39),
            ("BATTLEGROUND_QUEST_REWARD", 40),
            ("BATTLEGROUND_SPELL", 42),
            ("BATTLEGROUND_ANOMALY", 43),
            ("BATTLEGROUND_TRINKET", 44),
            ("PET", 45),
        ],
        "STEP" | "NEXT_STEP" => &[
            ("INVALID", 0),
            ("BEGIN_FIRST", 1),
            ("BEGIN_SHUFFLE", 2),
            ("BEGIN_DRAW", 3),
            ("BEGIN_MULLIGAN", 4),
            ("MAIN_BEGIN", 5),
            ("MAIN_READY", 6),
            ("MAIN_RESOURCE", 7),
            ("MAIN_DRAW", 8),
            ("MAIN_START", 9),
            ("MAIN_ACTION", 10),
            ("MAIN_COMBAT", 11),
            ("MAIN_END", 12),
            ("MAIN_NEXT", 13),
            ("FINAL_WRAPUP", 14),
            ("FINAL_GAMEOVER", 15),
            ("MAIN_CLEANUP", 16),
            ("MAIN_START_TRIGGERS", 17),
        ],
        "PLAYSTATE" => &[
            ("INVALID", 0),
            ("PLAYING", 1),
            ("WINNING", 2),
            ("LOSING", 3),
            ("WON", 4),
            ("LOST", 5),
            ("TIED", 6),
            ("DISCONNECTED", 7),
            ("CONCEDED", 8),
        ],
        _ => return None,
    };
    table.iter().find(|(n, _)| *n == value).map(|(_, v)| *v)
}
