//! The model a provider catalog recommends when nothing names one: a new agent, a composer
//! without a remembered model, or a dispatch whose model this device's provider does not offer.
//!
//! Provider defaults follow whatever was last chosen in the provider's own tool, so they are never
//! recommended. Anthropic catalogs prefer Opus, then Fable, Sonnet, and Haiku; OpenAI catalogs
//! prefer Sol, then Astra, Terra, and Luna. Each tier takes its newest version, and plain
//! variants win over fast, low-effort, or maximum-effort ones because they balance cost and
//! capability.

use ganbaru_chat_contracts::models::{ModelAvailability, ModelId, ProviderModel};
use std::cmp::Reverse;

/// Anthropic tiers in recommendation order: the balanced flagship first, then by capability.
const ANTHROPIC_TIERS: &[&str] = &["opus", "fable", "sonnet", "haiku"];
/// OpenAI tiers in recommendation order: the balanced flagship first, then by capability.
const OPENAI_TIERS: &[&str] = &["sol", "astra", "terra", "luna"];
/// Words that name a company's models without naming a tier.
const ANTHROPIC_MARKERS: &[&str] = &["anthropic", "claude"];
const OPENAI_MARKERS: &[&str] = &["openai", "gpt", "chatgpt", "codex", "o1", "o3", "o4"];
const XAI_MARKERS: &[&str] = &["xai", "grok"];
/// Model ids that route to whatever the provider decides instead of naming a model.
const ROUTE_IDS: &[&str] = &["default", "auto"];
/// Variant words that trade the balanced setting for speed, price, or maximum effort.
const VARIANT_WORDS: &[&str] = &[
    "fast", "max", "xhigh", "low", "thinking", "mini", "nano", "lite", "flash", "spark",
];

/// The company a provider family runs natively, whose models a mixed catalog prefers first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCompany {
    Anthropic,
    OpenAi,
    Xai,
    /// A provider that serves several companies without one of its own to prefer.
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Company {
    Anthropic,
    OpenAi,
    Xai,
    Other,
}

/// The id of the model `models` recommends for a provider that natively runs `native` models,
/// or `None` when the catalog offers nothing selectable.
pub fn recommended_model_id(native: NativeCompany, models: &[ProviderModel]) -> Option<ModelId> {
    let selectable: Vec<&ProviderModel> = models
        .iter()
        .filter(|model| {
            matches!(
                model.availability,
                ModelAvailability::Available
                    | ModelAvailability::Stale
                    | ModelAvailability::Unknown
            ) && !ROUTE_IDS.contains(&model.id.as_str().to_ascii_lowercase().as_str())
        })
        .collect();
    let built_in: Vec<&ProviderModel> = selectable
        .iter()
        .copied()
        .filter(|model| !model.custom)
        .collect();
    let pool = if built_in.is_empty() {
        selectable
    } else {
        built_in
    };
    pool.into_iter()
        .enumerate()
        .min_by_key(|(index, model)| rank(native, model, *index))
        .map(|(_, model)| model.id.clone())
}

type Rank = (usize, usize, Reverse<Vec<u64>>, usize, usize, usize);

fn rank(native: NativeCompany, model: &ProviderModel, index: usize) -> Rank {
    let words = words(model);
    let company = company(&words);
    let tier = match company {
        Company::Anthropic => tier_rank(&words, ANTHROPIC_TIERS),
        Company::OpenAi => tier_rank(&words, OPENAI_TIERS),
        Company::Xai | Company::Other => 0,
    };
    // Unknown companies keep the provider's order, since their versions say nothing comparable.
    let version = if company == Company::Other {
        Vec::new()
    } else {
        version(model)
    };
    let variants = words
        .iter()
        .filter(|word| VARIANT_WORDS.contains(&word.as_str()))
        .count();
    let extra = words.iter().filter(|word| !is_identity_word(word)).count();
    (
        company_rank(native, company),
        tier,
        Reverse(version),
        variants,
        extra,
        index,
    )
}

fn company_rank(native: NativeCompany, company: Company) -> usize {
    let native = match native {
        NativeCompany::Anthropic => Some(Company::Anthropic),
        NativeCompany::OpenAi => Some(Company::OpenAi),
        NativeCompany::Xai => Some(Company::Xai),
        NativeCompany::None => None,
    };
    if native == Some(company) {
        return 0;
    }
    match company {
        Company::Anthropic => 1,
        Company::OpenAi => 2,
        Company::Xai => 3,
        Company::Other => 4,
    }
}

fn tier_rank(words: &[String], tiers: &[&str]) -> usize {
    tiers
        .iter()
        .position(|tier| words.iter().any(|word| word == tier))
        .unwrap_or(tiers.len())
}

fn company(words: &[String]) -> Company {
    let has = |list: &[&str]| words.iter().any(|word| list.contains(&word.as_str()));
    if has(ANTHROPIC_MARKERS) || has(ANTHROPIC_TIERS) {
        Company::Anthropic
    } else if has(OPENAI_MARKERS) {
        Company::OpenAi
    } else if has(XAI_MARKERS) {
        Company::Xai
    } else {
        Company::Other
    }
}

fn is_identity_word(word: &str) -> bool {
    word.chars().all(|character| character.is_ascii_digit())
        || [
            ANTHROPIC_MARKERS,
            ANTHROPIC_TIERS,
            OPENAI_MARKERS,
            OPENAI_TIERS,
            XAI_MARKERS,
        ]
        .iter()
        .any(|list| list.contains(&word))
}

/// The lowercase alphanumeric words of a model's id and display name.
fn words(model: &ProviderModel) -> Vec<String> {
    format!("{} {}", model.id.as_str(), model.display_name)
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// The first version in a model's id or display name, such as `[5, 5]` for `claude-opus-5-5`
/// or for an `opus` alias named `Opus 5.5`.
fn version(model: &ProviderModel) -> Vec<u64> {
    let identity = format!("{} {}", model.id.as_str(), model.display_name);
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut previous_separator = true;
    for character in identity.chars() {
        if character.is_ascii_digit() && (previous_separator || !current.is_empty()) {
            current.push(character);
            continue;
        }
        if !current.is_empty() {
            parts.push(current.parse().unwrap_or(u64::MAX));
            current.clear();
            if (character == '.' || character == '-') && parts.len() < 3 {
                previous_separator = true;
                continue;
            }
            return parts;
        }
        if !parts.is_empty() {
            return parts;
        }
        previous_separator = !character.is_ascii_alphanumeric();
    }
    if !current.is_empty() {
        parts.push(current.parse().unwrap_or(u64::MAX));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_chat_contracts::models::ModelAvailability;

    fn model(id: &str, name: &str) -> ProviderModel {
        ProviderModel {
            id: ModelId::new(id.to_string()).unwrap(),
            display_name: name.to_string(),
            description: None,
            context_limit: None,
            availability: ModelAvailability::Available,
            capabilities: Vec::new(),
            options: Vec::new(),
            custom: false,
        }
    }

    fn recommended(native: NativeCompany, models: &[ProviderModel]) -> Option<String> {
        recommended_model_id(native, models).map(|id| id.as_str().to_string())
    }

    #[test]
    fn claude_recommends_the_newest_opus_over_fable_and_the_default_route() {
        let models = [
            model("default", "Default (recommended)"),
            model("fable", "Fable 5.2"),
            model("claude-opus-5", "Opus 5"),
            model("opus", "Opus 5.5"),
            model("sonnet", "Sonnet 5.5"),
            model("haiku", "Haiku 5.5"),
        ];
        assert_eq!(
            recommended(NativeCompany::Anthropic, &models).as_deref(),
            Some("opus")
        );
    }

    #[test]
    fn claude_falls_back_from_the_strongest_remaining_tier_down() {
        let fable = [
            model("haiku", "Haiku 5.5"),
            model("fable", "Fable 5.2"),
            model("sonnet", "Sonnet 5.5"),
        ];
        assert_eq!(
            recommended(NativeCompany::Anthropic, &fable).as_deref(),
            Some("fable")
        );
        let sonnet = [model("haiku", "Haiku 5.5"), model("sonnet", "Sonnet 5.5")];
        assert_eq!(
            recommended(NativeCompany::Anthropic, &sonnet).as_deref(),
            Some("sonnet")
        );
    }

    #[test]
    fn codex_recommends_the_newest_sol_then_astra() {
        let models = [
            model("gpt-5.6-luna", "GPT-5.6-Luna"),
            model("gpt-5.6-astra", "GPT-5.6-Astra"),
            model("gpt-5.5-sol", "GPT-5.5-Sol"),
            model("gpt-5.6-sol", "GPT-5.6-Sol"),
            model("gpt-5.6-terra", "GPT-5.6-Terra"),
        ];
        assert_eq!(
            recommended(NativeCompany::OpenAi, &models).as_deref(),
            Some("gpt-5.6-sol")
        );
        assert_eq!(
            recommended(NativeCompany::OpenAi, &models[..3]).as_deref(),
            Some("gpt-5.5-sol")
        );
        assert_eq!(
            recommended(NativeCompany::OpenAi, &models[..2]).as_deref(),
            Some("gpt-5.6-astra")
        );
    }

    #[test]
    fn a_mixed_catalog_prefers_the_balanced_anthropic_variant_and_never_auto() {
        let models = [
            model("auto", "Auto"),
            model("composer-2.5", "Composer 2.5"),
            model("gpt-5.6-sol-high", "GPT-5.6 Sol 1M High"),
            model("claude-opus-5-5-low", "Claude Opus 5.5 1M Low"),
            model("claude-opus-5-5-medium", "Claude Opus 5.5 1M"),
            model("claude-opus-5-5-medium-fast", "Claude Opus 5.5 1M Fast"),
            model("claude-opus-5-5-max", "Claude Opus 5.5 1M Max"),
            model("claude-opus-5-high", "Claude Opus 5 1M"),
        ];
        assert_eq!(
            recommended(NativeCompany::None, &models).as_deref(),
            Some("claude-opus-5-5-medium")
        );
        assert_eq!(
            recommended(NativeCompany::None, &models[..3]).as_deref(),
            Some("gpt-5.6-sol-high")
        );
        assert_eq!(
            recommended(NativeCompany::None, &models[..2]).as_deref(),
            Some("composer-2.5")
        );
    }

    #[test]
    fn grok_prefers_its_newest_native_model() {
        let models = [
            model("claude-opus-5-5", "Claude Opus 5.5"),
            model("grok-4.6", "Grok 4.6"),
            model("grok-4.7", "Grok 4.7"),
        ];
        assert_eq!(
            recommended(NativeCompany::Xai, &models).as_deref(),
            Some("grok-4.7")
        );
    }

    #[test]
    fn skips_unusable_and_custom_models_unless_only_custom_ones_remain() {
        let mut unavailable = model("opus", "Opus 5.5");
        unavailable.availability = ModelAvailability::Unavailable;
        let mut custom = model("claude-opus-9", "Opus 9");
        custom.custom = true;
        let models = [unavailable, custom.clone(), model("sonnet", "Sonnet 5.5")];
        assert_eq!(
            recommended(NativeCompany::Anthropic, &models).as_deref(),
            Some("sonnet")
        );
        assert_eq!(
            recommended(NativeCompany::Anthropic, &[custom]).as_deref(),
            Some("claude-opus-9")
        );
        assert_eq!(
            recommended(NativeCompany::Anthropic, &[model("default", "Default")]),
            None
        );
    }

    #[test]
    fn reads_the_first_version_of_an_id_or_name() {
        assert_eq!(
            version(&model("claude-opus-5-5-medium", "Claude Opus 5.5 1M")),
            [5, 5]
        );
        assert_eq!(version(&model("opus", "Opus 5.5")), [5, 5]);
        assert_eq!(version(&model("gpt-5.6-sol", "GPT-5.6-Sol")), [5, 6]);
        assert_eq!(
            version(&model("claude-haiku-4-5-20251001", "Haiku 4.5")),
            [4, 5, 20251001]
        );
        assert_eq!(version(&model("composer", "Composer")), Vec::<u64>::new());
    }
}
