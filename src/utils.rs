use hashbrown::HashSet;

pub fn class_to_combat_log(class: &str) -> HashSet<Box<str>> {
    let name1 = "npc_dota_hero_".to_string() + &class["CDOTA_Unit_Hero_".len()..].to_lowercase();

    let name2 = "npc_dota_hero".to_string()
        + &class["CDOTA_Unit_Hero_".len()..]
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_uppercase() {
                    format!("_{}", c.to_lowercase())
                } else {
                    c.to_string()
                }
            })
            .collect::<String>();

    let mut set = HashSet::default();
    set.insert(name1.into_boxed_str());
    set.insert(name2.into_boxed_str());

    set
}