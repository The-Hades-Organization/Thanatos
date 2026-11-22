//! Sugestão do post-mortem no watch (`src/lib/thanatos.ts:406-433`).

use serde_json::Value;

/// `RULE_PATTERN_MAX` (`src/lib/thanatos.ts:410`), em unidades UTF-16 como o `.length` do JS.
const RULE_PATTERN_MAX: usize = 200;

/// `FixRule` (`src/lib/thanatos.ts:409`).
#[derive(Clone, Debug)]
pub struct FixRule {
    pub branch: Option<regress::Regex>,
    pub labels: Vec<String>,
}

/// `fixRuleOf` (`src/lib/thanatos.ts:413`): `thanatos.fixBranchPattern` (regex JS, sem caixa) e `thanatos.fixLabels` do
/// perfil; sem nenhum dos dois, `None` (não sugere). Regex inválida ou com mais de 200 unidades não vale.
pub fn fix_rule_of(profile: &Value) -> Option<FixRule> {
    let section = profile.as_object()?.get("thanatos")?.as_object()?;
    let labels: Vec<String> = section
        .get("fixLabels")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let branch = section
        .get("fixBranchPattern")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty() && p.encode_utf16().count() <= RULE_PATTERN_MAX)
        .and_then(|p| regress::Regex::with_flags(p, "i").ok());
    (branch.is_some() || !labels.is_empty()).then_some(FixRule { branch, labels })
}

/// `isProductionFix` (`src/lib/thanatos.ts:428`).
pub fn is_production_fix(rule: Option<&FixRule>, branch: &str, labels: &[String]) -> bool {
    let Some(rule) = rule else {
        return false;
    };
    if rule
        .branch
        .as_ref()
        .is_some_and(|re| re.find(branch).is_some())
    {
        return true;
    }
    let have: Vec<String> = labels.iter().map(|l| l.to_lowercase()).collect();
    rule.labels.iter().any(|l| have.contains(&l.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labels(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn regra_casa_por_branch_sem_caixa_ou_por_label() {
        let rule = fix_rule_of(&json!({
            "thanatos": { "fixBranchPattern": "^(hotfix|fix)/", "fixLabels": ["Incident"] }
        }));
        let rule = rule.as_ref();
        assert!(is_production_fix(rule, "Hotfix/pagamento", &[]));
        assert!(is_production_fix(rule, "feat/x", &labels(&["incident"])));
        assert!(!is_production_fix(rule, "feat/x", &labels(&["bug"])));
        assert!(fix_rule_of(&json!({})).is_none());
        assert!(fix_rule_of(&json!({ "thanatos": {} })).is_none());
        assert!(!is_production_fix(None, "fix/x", &labels(&["incident"])));
    }

    #[test]
    fn regex_invalida_ou_grande_demais_nao_vale() {
        assert!(fix_rule_of(&json!({ "thanatos": { "fixBranchPattern": "(" } })).is_none());
        let grande = "a".repeat(201);
        assert!(fix_rule_of(&json!({ "thanatos": { "fixBranchPattern": grande } })).is_none());
        let limite = "a".repeat(200);
        assert!(fix_rule_of(&json!({ "thanatos": { "fixBranchPattern": limite } })).is_some());
    }

    #[test]
    fn labels_vazias_e_nao_texto_sao_descartadas() {
        assert!(fix_rule_of(&json!({ "thanatos": { "fixLabels": ["", 3, null] } })).is_none());
        let rule = fix_rule_of(&json!({ "thanatos": { "fixLabels": ["", "bug", 3] } })).unwrap();
        assert_eq!(rule.labels, vec!["bug".to_owned()]);
        assert!(rule.branch.is_none());
    }

    #[test]
    fn secao_com_tipo_errado_nao_gera_regra() {
        assert!(fix_rule_of(&json!({ "thanatos": "x" })).is_none());
        assert!(fix_rule_of(&json!({ "thanatos": null })).is_none());
        assert!(fix_rule_of(&json!("x")).is_none());
        assert!(fix_rule_of(&json!({ "thanatos": { "fixBranchPattern": 3 } })).is_none());
    }
}
