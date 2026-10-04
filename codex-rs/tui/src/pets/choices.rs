use super::catalog;
use super::model::CUSTOM_PET_PREFIX;
use super::model::Pet;
use super::model::custom_pet_selector;
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) struct PetChoice {
    pub(crate) id: String,
    pub(crate) legacy_id: Option<String>,
    pub(crate) name: String,
    pub(crate) description: String,
}

pub(crate) fn available_pet_choices(codex_home: &Path) -> Vec<PetChoice> {
    let mut choices = vec![PetChoice {
        id: super::DISABLED_PET_ID.into(),
        legacy_id: None,
        name: "Hide terminal pet".into(),
        description: "Use the full workspace width".into(),
    }];
    choices.extend(catalog::BUILTIN_PETS.iter().map(|pet| PetChoice {
        id: pet.id.into(),
        legacy_id: None,
        name: pet.display_name.into(),
        description: pet.description.into(),
    }));
    let mut custom = BTreeMap::new();
    for (directory, manifest) in [("avatars", "avatar.json"), ("pets", "pet.json")] {
        let Ok(entries) = std::fs::read_dir(codex_home.join(directory)) else {
            continue;
        };
        for entry in entries.flatten().take(512) {
            let path = entry.path();
            let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if id == super::DISABLED_PET_ID
                || id.starts_with(CUSTOM_PET_PREFIX)
                || !path.join(manifest).is_file()
            {
                continue;
            }
            let selector = custom_pet_selector(id);
            if let Ok(pet) = Pet::load_with_codex_home(&selector, Some(codex_home)) {
                custom.insert(
                    selector.clone(),
                    PetChoice {
                        id: selector,
                        legacy_id: Some(id.into()),
                        name: pet.display_name,
                        description: pet.description,
                    },
                );
            }
        }
    }
    choices.extend(custom.into_values());
    choices
}
