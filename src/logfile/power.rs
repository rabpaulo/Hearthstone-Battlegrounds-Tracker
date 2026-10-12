//! Line-level parser for `Power.log`.
//!
//! Only the `PowerTaskList.DebugPrintPower()` stream is used for game state:
//! it is what the client has actually played back, so boards seen through it
//! match what is on screen. `GameState.DebugPrintGame()` is read too, because
//! it is the only place that carries the game type and player names.

/// How a log line refers to an entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityRef {
    Id(i32),
    GameEntity,
    /// A player referenced by its display name (e.g. `Paul0#11623`).
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PowerLine {
    CreateGame,
    GameEntity { id: i32 },
    PlayerEntity { id: i32, player_id: i32 },
    FullEntity { id: i32, card_id: String },
    ShowEntity { entity: EntityRef, card_id: String },
    ChangeEntity { entity: EntityRef, card_id: String },
    HideEntity { entity: EntityRef, tag: String, value: String },
    TagChange { entity: EntityRef, tag: String, value: String },
    /// Indented `tag=X value=Y` line belonging to the entity declared above.
    Tag { tag: String, value: String },
    BlockStart { block_type: String, entity: EntityRef },
    BlockEnd,
    /// `GameState.DebugPrintGame() - GameType=GT_BATTLEGROUNDS`
    GameType(String),
    /// `GameState.DebugPrintGame() - PlayerID=8, PlayerName=Foo#1234`
    PlayerName { player_id: i32, name: String },
}

const POWER_MARKER: &str = "PowerTaskList.DebugPrintPower() - ";
const GAME_MARKER: &str = "GameState.DebugPrintGame() - ";

/// Parses one raw log line. Returns `None` for lines that carry nothing the
/// tracker uses.
pub fn parse_line(line: &str) -> Option<PowerLine> {
    if let Some(pos) = line.find(POWER_MARKER) {
        return parse_power(line[pos + POWER_MARKER.len()..].trim());
    }
    if let Some(pos) = line.find(GAME_MARKER) {
        return parse_game_info(line[pos + GAME_MARKER.len()..].trim());
    }
    None
}

fn parse_game_info(s: &str) -> Option<PowerLine> {
    if let Some(t) = s.strip_prefix("GameType=") {
        return Some(PowerLine::GameType(t.to_string()));
    }
    let rest = s.strip_prefix("PlayerID=")?;
    let (id, rest) = rest.split_once(", PlayerName=")?;
    Some(PowerLine::PlayerName {
        player_id: id.parse().ok()?,
        name: rest.to_string(),
    })
}

fn parse_power(s: &str) -> Option<PowerLine> {
    if let Some(rest) = s.strip_prefix("tag=") {
        let (tag, value) = rest.split_once(" value=")?;
        return Some(PowerLine::Tag { tag: tag.to_string(), value: first_word(value).to_string() });
    }
    if let Some(rest) = s.strip_prefix("TAG_CHANGE Entity=") {
        let (entity, rest) = split_entity(rest)?;
        let rest = rest.trim_start().strip_prefix("tag=")?;
        let (tag, value) = rest.split_once(" value=")?;
        return Some(PowerLine::TagChange {
            entity,
            tag: tag.to_string(),
            value: first_word(value).to_string(),
        });
    }
    if s == "BLOCK_END" {
        return Some(PowerLine::BlockEnd);
    }
    if let Some(rest) = s.strip_prefix("BLOCK_START ") {
        let bt = rest.find("BlockType=")?;
        let block_type = first_word(&rest[bt + "BlockType=".len()..]).to_string();
        let en = rest.find(" Entity=")?;
        let (entity, _) = split_entity(&rest[en + " Entity=".len()..])?;
        return Some(PowerLine::BlockStart { block_type, entity });
    }
    if let Some(rest) = s.strip_prefix("FULL_ENTITY - ") {
        let card_id = card_id_suffix(rest);
        if let Some(r) = rest.strip_prefix("Creating ID=") {
            let id = first_word(r).parse().ok()?;
            return Some(PowerLine::FullEntity { id, card_id });
        }
        let r = rest.strip_prefix("Updating ")?;
        let (entity, _) = split_entity(r)?;
        let EntityRef::Id(id) = entity else { return None };
        return Some(PowerLine::FullEntity { id, card_id });
    }
    if let Some(rest) = s.strip_prefix("SHOW_ENTITY - Updating Entity=") {
        let (entity, _) = split_entity(rest)?;
        return Some(PowerLine::ShowEntity { entity, card_id: card_id_suffix(rest) });
    }
    if let Some(rest) = s.strip_prefix("CHANGE_ENTITY - Updating Entity=") {
        let (entity, _) = split_entity(rest)?;
        return Some(PowerLine::ChangeEntity { entity, card_id: card_id_suffix(rest) });
    }
    if let Some(rest) = s.strip_prefix("HIDE_ENTITY - Entity=") {
        let (entity, rest) = split_entity(rest)?;
        let rest = rest.trim_start().strip_prefix("tag=")?;
        let (tag, value) = rest.split_once(" value=")?;
        return Some(PowerLine::HideEntity {
            entity,
            tag: tag.to_string(),
            value: first_word(value).to_string(),
        });
    }
    if let Some(rest) = s.strip_prefix("GameEntity EntityID=") {
        return Some(PowerLine::GameEntity { id: first_word(rest).parse().ok()? });
    }
    if let Some(rest) = s.strip_prefix("Player EntityID=") {
        let id = first_word(rest).parse().ok()?;
        let p = rest.find("PlayerID=")?;
        let player_id = first_word(&rest[p + "PlayerID=".len()..]).parse().ok()?;
        return Some(PowerLine::PlayerEntity { id, player_id });
    }
    if s == "CREATE_GAME" {
        return Some(PowerLine::CreateGame);
    }
    None
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

/// `... CardID=XYZ` at the end of FULL/SHOW/CHANGE_ENTITY lines.
fn card_id_suffix(s: &str) -> String {
    s.rfind("CardID=")
        .map(|p| first_word(&s[p + "CardID=".len()..]).to_string())
        .unwrap_or_default()
}

/// Splits an entity reference off the front of `s`, returning it and the
/// remainder. Bracketed references may nest (`UNKNOWN ENTITY [cardType=…]`).
fn split_entity(s: &str) -> Option<(EntityRef, &str)> {
    if s.starts_with('[') {
        let mut depth = 0usize;
        for (i, c) in s.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        let inner = &s[1..i];
                        let id = bracket_id(inner)?;
                        return Some((EntityRef::Id(id), &s[i + 1..]));
                    }
                }
                _ => {}
            }
        }
        return None;
    }
    // Plain reference: up to the next keyword. Player names can contain
    // spaces ("Bartender Bob"), so split on the keyword, not on whitespace.
    let end = [" tag=", " EffectCardId=", " CardID="]
        .iter()
        .filter_map(|k| s.find(k))
        .min()
        .unwrap_or(s.len());
    let raw = s[..end].trim();
    let entity = if raw == "GameEntity" {
        EntityRef::GameEntity
    } else if let Ok(id) = raw.parse() {
        EntityRef::Id(id)
    } else {
        EntityRef::Name(raw.to_string())
    };
    Some((entity, &s[end..]))
}

fn bracket_id(inner: &str) -> Option<i32> {
    // `entityName=` comes first and may contain anything, so take the last
    // ` id=` occurrence, which is the structural one.
    let p = inner.rfind(" id=")?;
    let digits: String = inner[p + 4..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "D 02:11:18.0624663 PowerTaskList.DebugPrintPower() - ";

    #[test]
    fn tag_change_on_bracketed_entity() {
        let l = format!("{P}    TAG_CHANGE Entity=[entityName=Harmless Bonehead id=312 zone=PLAY zonePos=1 cardId=BG28_300 player=16] tag=ZONE value=REMOVEDFROMGAME ");
        assert_eq!(
            parse_line(&l),
            Some(PowerLine::TagChange {
                entity: EntityRef::Id(312),
                tag: "ZONE".into(),
                value: "REMOVEDFROMGAME".into()
            })
        );
    }

    #[test]
    fn tag_change_on_named_player_with_space() {
        let l = format!("{P}    TAG_CHANGE Entity=Bartender Bob tag=BACON_CURRENT_COMBAT_PLAYER_ID value=0 ");
        assert_eq!(
            parse_line(&l),
            Some(PowerLine::TagChange {
                entity: EntityRef::Name("Bartender Bob".into()),
                tag: "BACON_CURRENT_COMBAT_PLAYER_ID".into(),
                value: "0".into()
            })
        );
    }

    #[test]
    fn full_entity_variants() {
        let a = format!("{P}FULL_ENTITY - Creating ID=39 CardID=TB_BaconShop_HERO_PH");
        assert_eq!(parse_line(&a), Some(PowerLine::FullEntity { id: 39, card_id: "TB_BaconShop_HERO_PH".into() }));
        let b = format!("{P}    FULL_ENTITY - Updating [entityName=UNKNOWN ENTITY [cardType=INVALID] id=602 zone=PLAY zonePos=0 cardId= player=16] CardID=");
        assert_eq!(parse_line(&b), Some(PowerLine::FullEntity { id: 602, card_id: String::new() }));
    }

    #[test]
    fn show_entity_and_block() {
        let s = format!("{P}    SHOW_ENTITY - Updating Entity=[entityName=UNKNOWN ENTITY [cardType=INVALID] id=602 zone=PLAY zonePos=0 cardId= player=16] CardID=BG29_813e");
        assert_eq!(
            parse_line(&s),
            Some(PowerLine::ShowEntity { entity: EntityRef::Id(602), card_id: "BG29_813e".into() })
        );
        let b = format!("{P}BLOCK_START BlockType=ATTACK Entity=[entityName=Buzzing Vermin id=314 zone=PLAY zonePos=1 cardId=BG31_803 player=8] EffectCardId=System.Collections.Generic.List`1[System.String] EffectIndex=0 Target=0 SubOption=-1 ");
        assert_eq!(
            parse_line(&b),
            Some(PowerLine::BlockStart { block_type: "ATTACK".into(), entity: EntityRef::Id(314) })
        );
        let c = format!("{P}BLOCK_START BlockType=TRIGGER Entity=ZaneWall EffectCardId=System.Collections.Generic.List`1[System.String] EffectIndex=-1 Target=0 SubOption=-1 TriggerKeyword=TAG_NOT_SET");
        assert_eq!(
            parse_line(&c),
            Some(PowerLine::BlockStart { block_type: "TRIGGER".into(), entity: EntityRef::Name("ZaneWall".into()) })
        );
    }

    #[test]
    fn game_info_lines() {
        let g = "D 02:09:20.1016296 GameState.DebugPrintGame() - GameType=GT_BATTLEGROUNDS";
        assert_eq!(parse_line(g), Some(PowerLine::GameType("GT_BATTLEGROUNDS".into())));
        let p = "D 02:09:20.1016296 GameState.DebugPrintGame() - PlayerID=8, PlayerName=Paul0#11623";
        assert_eq!(parse_line(p), Some(PowerLine::PlayerName { player_id: 8, name: "Paul0#11623".into() }));
    }

    #[test]
    fn ignores_game_state_power_stream() {
        let l = "D 02:09:20.1016296 GameState.DebugPrintPower() -     TAG_CHANGE Entity=GameEntity tag=TURN value=1 ";
        assert_eq!(parse_line(l), None);
    }
}
