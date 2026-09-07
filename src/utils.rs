use hashbrown::HashSet;

pub fn class_to_combat_log(class: &str) -> HashSet<Box<str>> {
    let hero_name = class.strip_prefix("CDOTA_Unit_Hero_").unwrap_or(class);
    let name1 = "npc_dota_hero_".to_string() + &hero_name.to_lowercase();
    let snake_case = hero_name
        .chars()
        .enumerate()
        .flat_map(|(index, character)| {
            let separator = (index > 0 && character.is_ascii_uppercase()).then_some('_');
            separator.into_iter().chain(character.to_lowercase())
        })
        .collect::<String>();
    let name2 = "npc_dota_hero_".to_string() + &snake_case;

    let mut set = HashSet::default();
    set.insert(name1.into_boxed_str());
    set.insert(name2.into_boxed_str());

    set
}
