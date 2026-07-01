use serde::{Deserialize, Serialize};

const RECIPES_JSON: &str = include_str!("../recipes.json");

/// Prédéfinition de service embarquée (catalogue « recipes »).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub url: String,
    pub icon: String,
    #[serde(default)]
    pub user_agent: Option<String>,
}

/// Charge le catalogue embarqué, en filtrant les entrées invalides.
pub fn load_recipes() -> Vec<Recipe> {
    let recipes: Vec<Recipe> = serde_json::from_str(RECIPES_JSON).unwrap_or_default();
    recipes
        .into_iter()
        .filter(|r| {
            !r.id.is_empty()
                && !r.name.is_empty()
                && !r.icon.is_empty()
                && r.url.parse::<url::Url>().is_ok()
        })
        .map(|mut r| {
            if r.user_agent.as_deref() == Some("") {
                r.user_agent = None;
            }
            r
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_recipes_embedded() {
        let recipes = load_recipes();
        assert!(!recipes.is_empty(), "embedded catalog should not be empty");

        for recipe in &recipes {
            assert!(!recipe.id.is_empty());
            assert!(!recipe.name.is_empty());
            assert!(recipe.url.parse::<url::Url>().is_ok());
            assert!(!recipe.icon.is_empty());
        }

        let telegram = recipes.iter().find(|r| r.id == "telegram");
        assert!(telegram.is_some());
        assert_eq!(telegram.unwrap().name, "Telegram");
    }
}
